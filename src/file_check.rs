use std::ffi::OsStr;
use std::path::Path;

// 判断是否为图片文件
pub fn is_image_file(path: &Path) -> bool {
    let ext = path.extension().and_then(OsStr::to_str).unwrap_or("");
    matches!(ext.to_lowercase().as_str(), "png" | "jpg" | "jpeg" | "psd")
}

// 判断是否为支持的压缩文件格式
pub fn is_archive_file(path: &Path) -> bool {
    let ext = path.extension().and_then(OsStr::to_str).unwrap_or("");
    matches!(
        ext.to_lowercase().as_str(),
        "zip" | "rar" | "7z" | "r00" | "z01" | "001" | "part1.rar" | "zip.001"
    )
}

pub fn is_first_7z_volume(path: &Path) -> bool {
    path.extension() == Some(OsStr::new("001"))
        && path
            .file_stem()
            .and_then(|stem| Path::new(stem).extension())
            .and_then(OsStr::to_str)
            .is_some_and(|ext| ext.eq_ignore_ascii_case("7z"))
}

pub fn rar_volume_name(path: &Path) -> Option<(&str, usize)> {
    if !path.extension()?.to_str()?.eq_ignore_ascii_case("rar") {
        return None;
    }
    let (base, suffix) = path.file_stem()?.to_str()?.rsplit_once('.')?;
    let suffix = suffix.to_ascii_lowercase();
    let digits = suffix
        .strip_prefix("part")
        .or_else(|| suffix.strip_prefix("par"))?;
    if !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let number = digits.parse().ok()?;
    (number > 0).then_some((base, number))
}
