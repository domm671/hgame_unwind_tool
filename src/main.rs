use anyhow::{anyhow, Result};
use anyhow::Context;
use std::{
    ffi::OsStr,
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
    process::Command,
};

/// 主函数：解析参数并启动解压流程
fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let file_path = match args.get(1) {
        Some(path) => PathBuf::from(path),
        None => {
            // 无参数时提示用户输入文件路径
            print!("请输入压缩文件路径: ");
            io::stdout().flush()?;
            let mut input = String::new();
            io::stdin().read_line(&mut input)?;
            PathBuf::from(input.trim())
        }
    };

    // 初始化临时密码并启动解压循环
    let mut temp_password: Option<String> = None;
    let mut current_file = file_path.clone();

    loop {
        // 步骤1：检查是否为压缩文件（含分卷）或单一图片文件
        if !is_archive_file(&current_file) {
            println!("文件不是支持的压缩格式，接下来检查是否是图片格式");
            let check_dir = current_file.parent().unwrap_or_else(|| Path::new(".")).to_path_buf();
            let extracted_files = get_files_in_dir(&check_dir)?;
            let file_len = extracted_files.len();
            if !((file_len == 1) && is_image_file(&current_file)) {
                break; // 不是压缩文件且不是图片文件，结束流程
            }
            let file = &extracted_files[0];
            current_file = convert_to_archive(file)?;
            println!("文件已重命名为压缩包: {:?}", current_file);
        }

        // 步骤2：检查是否需要解压密码
        let password = if needs_password(&current_file)? {
            // 步骤3：需要密码时处理逻辑
            // 先尝试无密码解压
            if test_password(&current_file, "")? {
            None
            } else if let Some(pwd) = &temp_password {
            // 尝试临时密码
            if test_password(&current_file, pwd)? {
                Some(pwd.clone())
            } else {
                // 临时密码无效时请求用户输入
                println!("临时密码无效");
                get_password(&current_file, Some(pwd))?
            }
            } else {
            // 无临时密码时请求用户输入
            println!("压缩包需要密码");
            get_password(&current_file, None)?
            }
        } else {
            None
        };

        // 步骤4：更新临时密码（首次成功时保存）
        if password.is_some() && temp_password.is_none() {
            temp_password = password.clone();
        }

        // 步骤5：解压文件。如果被解压文件和解压后文件因同名同格式，被解压文件会被覆盖
        let output_dir = extract_archive(&current_file, password.as_deref())?;

        let extracted_files = get_files_in_dir(&output_dir)?;
        // 步骤6：删除原始压缩包
        let has_dirs = fs::read_dir(&output_dir)?
            .any(|entry| entry.ok()
            .map(|e| e.path().is_dir())
            .unwrap_or(false));
            
        if extracted_files.len() > 1 || extracted_files.is_empty() || has_dirs {
            fs::remove_file(&current_file)
            .context("删除原始压缩文件失败")?;
            println!("已删除原始文件: {:?}", current_file);
        }

        let extracted_files = get_files_in_dir(&output_dir)?;
        // 步骤7：检查解压结果
        println!("解压后的文件列表: {:?}", extracted_files);
        if extracted_files.len() > 1 || extracted_files.is_empty() {
            println!("解压后有多个文件或无文件，结束流程");
            break; // 多个文件结束流程
        }
        current_file = extracted_files[0].clone(); // 更新当前文件为解压后的单一文件
        println!("解压后的文件: {:?}", current_file);
    }

    println!("解压流程完成");
    Ok(())
}

/// 判断是否为支持的压缩文件格式
fn is_archive_file(path: &Path) -> bool {
    let ext = path.extension().and_then(OsStr::to_str).unwrap_or("");
    matches!(
        ext.to_lowercase().as_str(),
        "zip" | "rar" | "7z" | "r00" | "z01" | "001" | "part1.rar" | "zip.001"
    )
}

/// 判断是否为图片文件
fn is_image_file(path: &Path) -> bool {
    let ext = path.extension().and_then(OsStr::to_str).unwrap_or("");
    matches!(ext.to_lowercase().as_str(), "png" | "jpg" | "jpeg" | "psd")
}

/// 检查压缩包是否需要密码
fn needs_password(archive: &Path) -> Result<bool> {
    let output = Command::new("7z")
        .arg("l")
        .arg(archive)
        .output()
        .context("执行7z命令失败，请确保已安装7-Zip")?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    Ok(stdout.contains("Encrypted") || !output.status.success())
}

/// 测试密码是否正确
fn test_password(archive: &Path, password: &str) -> Result<bool> {
    let status = Command::new("7z")
        .arg("t")
        .arg(format!("-p{}", password))
        .arg(archive)
        .status()
        .context("测试密码失败")?;

    Ok(status.success())
}

/// 获取用户输入的密码
fn get_password(archive: &Path, temp_pwd: Option<&str>) -> Result<Option<String>> {
    if temp_pwd.is_some() {
        println!("临时密码无效，请重新输入");
    } else {
        println!("压缩包需要密码: {:?}", archive);
    }

    print!("请输入解压密码（直接回车跳过）: ");
    io::stdout().flush()?;
    let mut password = String::new();
    io::stdin().read_line(&mut password)?;

    Ok(Some(password.trim().to_string()).filter(|s| !s.is_empty()))
}

/// 解压压缩文件到当前目录
fn extract_archive(archive: &Path, password: Option<&str>) -> Result<PathBuf> {
    let output_dir = archive.parent().unwrap_or_else(|| Path::new(".")).to_path_buf();

    let mut cmd = Command::new("7z");
    cmd.arg("x").arg(archive);

    if let Some(pwd) = password {
        cmd.arg(format!("-p{}", pwd));
    }

    // 设置输出目录为当前目录
    cmd.arg(format!("-o{}", output_dir.display()));

    let status = cmd.status().context("解压命令执行失败")?;
    if !status.success() {
        return Err(anyhow!("解压失败，退出状态: {}", status));
    }

    Ok(output_dir)
}

/// 获取目录中的文件列表（忽略子目录）
fn get_files_in_dir(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    for entry in fs::read_dir(dir).context("读取解压目录失败")? {
        let entry = entry?;
        let path = entry.path();
        if path.is_file() {
            files.push(path);
        }
    }
    Ok(files)
}

/// 将图片文件转换为压缩包
fn convert_to_archive(image_path: &Path) -> Result<PathBuf> {
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