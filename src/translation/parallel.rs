use super::{TranslationProfile, provider_for};
use anyhow::{Result, bail};
use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
    thread,
};
use tracing::warn;

const MAX_PARALLEL_REQUESTS: usize = 4;

pub(in crate::translation) fn translate_blocks_parallel(
    profile: &TranslationProfile,
    texts: &[String],
    timeout_ms: u64,
) -> Result<Vec<String>> {
    if texts.is_empty() {
        return Ok(Vec::new());
    }

    let worker_count = MAX_PARALLEL_REQUESTS.min(texts.len());
    let queue = Arc::new(Mutex::new((0..texts.len()).collect::<VecDeque<_>>()));
    let outputs = Arc::new(Mutex::new(vec![None::<String>; texts.len()]));
    let errors = Arc::new(Mutex::new(Vec::<String>::new()));
    let source_texts = Arc::new(texts.to_vec());

    let mut handles = Vec::with_capacity(worker_count);
    for _ in 0..worker_count {
        let queue = Arc::clone(&queue);
        let outputs = Arc::clone(&outputs);
        let errors = Arc::clone(&errors);
        let source_texts = Arc::clone(&source_texts);
        let profile = profile.clone();

        handles.push(thread::spawn(move || {
            let provider = provider_for(profile.provider_kind);
            loop {
                let next_index = {
                    let Ok(mut q) = queue.lock() else {
                        return;
                    };
                    q.pop_front()
                };
                let Some(index) = next_index else {
                    break;
                };

                let source = source_texts[index].clone();
                match provider.translate_one(&profile, &source, timeout_ms) {
                    Ok(translated) => {
                        let value = normalize_translated_output(&translated, &source);
                        if let Ok(mut out) = outputs.lock() {
                            out[index] = Some(value);
                        }
                    }
                    Err(error) => {
                        if let Ok(mut errs) = errors.lock() {
                            errs.push(format!("块 #{} 失败: {}", index, error));
                        }
                        if let Ok(mut out) = outputs.lock() {
                            out[index] = Some(source);
                        }
                    }
                }
            }
        }));
    }

    for handle in handles {
        if handle.join().is_err() {
            bail!("翻译线程异常退出");
        }
    }

    let error_messages = errors.lock().map(|errs| errs.clone()).unwrap_or_default();
    if error_messages.len() == texts.len() && !error_messages.is_empty() {
        bail!("所有文本块翻译失败；{}", error_messages[0]);
    }
    if !error_messages.is_empty() {
        warn!(
            failed = error_messages.len(),
            total = texts.len(),
            first_error = %error_messages[0],
            "partial translation failure"
        );
    }

    let result = outputs
        .lock()
        .map(|items| {
            items
                .iter()
                .enumerate()
                .map(|(index, item)| item.clone().unwrap_or_else(|| texts[index].clone()))
                .collect::<Vec<_>>()
        })
        .unwrap_or_else(|_| texts.to_vec());
    Ok(result)
}

fn normalize_translated_output(translated: &str, source: &str) -> String {
    let trimmed = translated.trim();
    if trimmed.is_empty() {
        source.to_string()
    } else {
        trimmed.to_string()
    }
}
