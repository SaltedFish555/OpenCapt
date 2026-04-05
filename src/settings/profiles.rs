pub(super) const BAIDU_TRANSLATION_SOURCE_LANG_OPTIONS: [(&str, &str); 9] = [
    ("auto", "自动检测"),
    ("zh", "中文"),
    ("en", "英语"),
    ("jp", "日语"),
    ("kor", "韩语"),
    ("fra", "法语"),
    ("spa", "西班牙语"),
    ("ru", "俄语"),
    ("de", "德语"),
];
pub(super) const BAIDU_TRANSLATION_TARGET_LANG_OPTIONS: [(&str, &str); 8] = [
    ("zh", "中文"),
    ("en", "英语"),
    ("jp", "日语"),
    ("kor", "韩语"),
    ("fra", "法语"),
    ("spa", "西班牙语"),
    ("ru", "俄语"),
    ("de", "德语"),
];

pub(super) fn baidu_lang_label(options: &[(&str, &str)], value: &str, empty_label: &str) -> String {
    let trimmed = value.trim();
    options
        .iter()
        .find(|(code, _)| *code == trimmed)
        .map(|(_, label)| format!("{} ({})", label, trimmed))
        .unwrap_or_else(|| {
            if trimmed.is_empty() {
                empty_label.to_string()
            } else {
                trimmed.to_string()
            }
        })
}
