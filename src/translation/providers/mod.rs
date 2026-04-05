mod baidu;
mod openai;

pub(in crate::translation) use baidu::BaiduImageTranslationProvider;
pub(in crate::translation) use openai::OpenAiCompatibleTranslationProvider;
