//! 编码与换行处理。
//!
//! 旧工具的行为（实测确认）：
//! - `.pb.h` 写为 **GB2312**（实际是 GBK/codepage 936），CRLF 换行
//! - `.options` 写为 **ASCII**，CRLF 换行
//! - `.proto` 源文件是 UTF-8

/// 编码目标
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Encoding {
    /// GB2312/GBK，兼容旧工具输出
    #[default]
    Gb2312,
    Utf8,
}

#[derive(Debug)]
pub struct EncodeError {
    pub message: String,
    /// 无法编码的字符（最多列 5 个）
    pub unmappable: Vec<char>,
}

impl std::fmt::Display for EncodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.unmappable.is_empty() {
            write!(f, "{}", self.message)
        } else {
            write!(
                f,
                "{}（无法用 GB2312 表示的字符: {:?}）",
                self.message, self.unmappable
            )
        }
    }
}

impl std::error::Error for EncodeError {}

/// 把文本编码成目标字节流。
///
/// `lossy` 为 false 时，遇到 GB2312 无法表示的字符会报错，
/// 而不是像旧工具那样静默写成 `?`。
pub fn encode(text: &str, enc: Encoding, lossy: bool) -> Result<Vec<u8>, EncodeError> {
    match enc {
        Encoding::Utf8 => Ok(text.as_bytes().to_vec()),
        Encoding::Gb2312 => {
            let (bytes, _, had_errors) = encoding_rs::GBK.encode(text);
            if had_errors && !lossy {
                let unmappable: Vec<char> = text
                    .chars()
                    .filter(|c| {
                        let mut buf = [0u8; 4];
                        let s: &str = c.encode_utf8(&mut buf);
                        let (_, _, err) = encoding_rs::GBK.encode(s);
                        err
                    })
                    .take(5)
                    .collect();
                return Err(EncodeError {
                    message: "输出包含 GB2312 无法表示的字符".to_string(),
                    unmappable,
                });
            }
            Ok(bytes.into_owned())
        }
    }
}

/// 把 GB2312 字节解码为文本（读取旧工具生成的文件时用）
pub fn decode(bytes: &[u8], enc: Encoding) -> String {
    match enc {
        Encoding::Utf8 => String::from_utf8_lossy(bytes).into_owned(),
        Encoding::Gb2312 => {
            let (text, _, _) = encoding_rs::GBK.decode(bytes);
            text.into_owned()
        }
    }
}

/// 统一换行符为 CRLF（旧工具用 `StreamWriter.WriteLine` 写 CRLF）
pub fn to_crlf(text: &str) -> String {
    let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
    normalized.replace('\n', "\r\n")
}

/// 统一换行符为 LF
pub fn to_lf(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\r', "\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gb2312_roundtrip_chinese() {
        let s = "/** 风扇状态 */";
        let bytes = encode(s, Encoding::Gb2312, false).unwrap();
        assert_eq!(decode(&bytes, Encoding::Gb2312), s);
    }

    #[test]
    fn gb2312_rejects_emoji() {
        let err = encode("状态 🚀", Encoding::Gb2312, false);
        assert!(err.is_err());
    }

    #[test]
    fn crlf_conversion() {
        assert_eq!(to_crlf("a\nb"), "a\r\nb");
        assert_eq!(to_crlf("a\r\nb"), "a\r\nb");
    }
}
