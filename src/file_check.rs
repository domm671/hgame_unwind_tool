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