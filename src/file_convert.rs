use std::path::{Path, PathBuf};
use std::fs;
use anyhow::{Result, Context};

// 将图片文件转换为压缩包
pub fn convert_to_archive(image_path: &Path) -> Result<PathBuf> {
    let archive_path = image_path.with_extension("").with_extension("rar");
    fs::rename(image_path, &archive_path).context("重命名文件失败")?;
    
    // 添加原始文件名前缀（流程图要求）
    let final_path = archive_path
        .parent()
        .unwrap()
        .join(format!("uc_{}", archive_path.file_name().unwrap().to_string_lossy()));
    fs::rename(&archive_path, &final_path).context("添加前缀失败")?;

    Ok(final_path)
}