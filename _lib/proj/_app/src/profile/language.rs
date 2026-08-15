use super::ProfileError;

pub const DEFAULT_LANGUAGE: &str = "zh-CN";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryLanguage {
    ZhCn,
    En,
}

impl EntryLanguage {
    pub fn parse(value: &str) -> Result<Self, ProfileError> {
        match value {
            "zh-CN" => Ok(Self::ZhCn),
            "en" => Ok(Self::En),
            _ => Err(ProfileError::new("language must be one of: zh-CN, en")),
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ZhCn => "zh-CN",
            Self::En => "en",
        }
    }

    pub const fn help_file_name(self) -> &'static str {
        match self {
            Self::ZhCn => "zh-CN.txt",
            Self::En => "en.txt",
        }
    }
}

impl Default for EntryLanguage {
    fn default() -> Self {
        Self::ZhCn
    }
}
