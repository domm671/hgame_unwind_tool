use anyhow::{anyhow, Context, Result};
use std::{
    collections::BTreeMap,
    ffi::OsString,
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

mod file_check;
use crate::file_check::{is_archive_file, is_image_file, rar_volume_name};

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let file_path = match args.get(1) {
        Some(path) => PathBuf::from(path),
        None => {
            print!("请输入压缩文件路径: ");
            io::stdout().flush()?;
            let mut input = String::new();
            io::stdin().read_line(&mut input)?;
            PathBuf::from(input.trim())
        }
    };

    if !is_archive_file(&file_path) && !is_image_file(&file_path) {
        return Err(anyhow!("不支持的压缩文件格式: {:?}", file_path));
    }

    let archive = std::path::absolute(file_path)?;
    // 同一目录的任务共用工作目录名，避免并发发布时互相覆盖。
    let work_dir = archive.with_file_name(".unzip-tool-work");
    fs::create_dir(&work_dir).with_context(|| format!("无法创建解压工作目录: {:?}", work_dir))?;

    let result = unpack_archive(&archive, &work_dir, &mut None);
    let cleanup = fs::remove_dir_all(&work_dir)
        .with_context(|| format!("无法清理解压工作目录: {:?}", work_dir));
    result?;
    cleanup?;
    println!("解压流程完成");
    Ok(())
}

fn unpack_archive(
    archive: &Path,
    work_dir: &Path,
    cached_password: &mut Option<String>,
) -> Result<()> {
    let (input, originals) = prepare_rar_volumes(archive, work_dir)?;
    let Some(info) = inspect_archive(&input, None)
        .with_context(|| format!("检查原始压缩包失败: {:?}", archive))?
    else {
        return Ok(());
    };
    let password = if info.encrypted {
        match cached_password {
            Some(password) if test_password(&input, password)? => Some(password.clone()),
            _ => Some(get_password(archive)?),
        }
    } else {
        None
    };

    let volume_count = if originals.len() > 1 {
        let count = match info.volumes {
            Some(count) => count,
            None => inspect_archive(&input, password.as_deref())?
                .and_then(|info| info.volumes)
                .context("无法确认 RAR 分卷数")?,
        };
        if count == 0 || count > originals.len() {
            return Err(anyhow!("7-Zip 报告的 RAR 分卷数与工作目录不符"));
        }
        count
    } else {
        1
    };

    let output_dir = create_available_dir(&work_dir.join("layer"))?;
    extract_archive(&input, &output_dir, password.as_deref())
        .with_context(|| format!("解压原始压缩包失败: {:?}", archive))?;
    if password.is_some() {
        *cached_password = password;
    }
    unpack_directory(&output_dir, work_dir, cached_password)?;
    publish_contents(&output_dir, archive)?;
    for original in originals.iter().take(volume_count).rev() {
        fs::remove_file(original)
            .with_context(|| format!("删除原始压缩文件失败: {:?}", original))?;
        println!("已删除原始文件: {:?}", original);
    }
    Ok(())
}

fn prepare_rar_volumes(archive: &Path, work_dir: &Path) -> Result<(PathBuf, Vec<PathBuf>)> {
    let mut originals = vec![archive.to_path_buf()];
    let Some((base, 1)) = rar_volume_name(archive) else {
        return Ok((archive.to_path_buf(), originals));
    };
    let parent = archive.parent().context("压缩包没有所在目录")?;
    let mut parts = BTreeMap::new();
    for entry in fs::read_dir(parent)? {
        let entry = entry?;
        let path = entry.path();
        let Some((name, number)) = rar_volume_name(&path) else {
            continue;
        };
        let same_base = name == base || (cfg!(windows) && name.eq_ignore_ascii_case(base));
        if number > 1 && same_base && entry.file_type()?.is_file() {
            // 编号有歧义时不给 7-Zip 提供该卷，避免随意选中其中一个。
            parts
                .entry(number)
                .and_modify(|part| *part = None)
                .or_insert(Some(path));
        }
    }
    if parts.is_empty() {
        return Ok((archive.to_path_buf(), originals));
    }
    originals.extend((2..).map_while(|number| parts.remove(&number).flatten()));

    // 7-Zip 按首卷的命名寻找后续卷。只在工作目录里统一名字。
    let aliases = create_available_dir(&work_dir.join("volumes"))?;
    for (index, original) in originals.iter().enumerate() {
        let alias = aliases.join(format!("archive.part{}.rar", index + 1));
        if fs::hard_link(original, &alias).is_err() {
            fs::copy(original, &alias)
                .with_context(|| format!("无法准备 RAR 分卷: {:?}", original))?;
        }
    }
    Ok((aliases.join("archive.part1.rar"), originals))
}

fn unpack_directory(
    dir: &Path,
    work_dir: &Path,
    cached_password: &mut Option<String>,
) -> Result<()> {
    // 解包会增删条目，先读完整个目录。
    let mut entries = fs::read_dir(dir)?.collect::<io::Result<Vec<_>>>()?;
    entries
        .sort_by_cached_key(|entry| rar_volume_name(&entry.path()).map_or(0, |(_, number)| number));
    for entry in entries {
        let path = entry.path();
        let metadata = match fs::symlink_metadata(&path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error.into()),
        };
        let file_type = metadata.file_type();
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;
            const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
            if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
                continue;
            }
        }
        if file_type.is_dir() {
            unpack_directory(&path, work_dir, cached_password)?;
        } else if file_type.is_file() && (is_archive_file(&path) || is_image_file(&path)) {
            unpack_archive(&path, work_dir, cached_password)?;
        }
    }
    Ok(())
}

struct ArchiveInfo {
    encrypted: bool,
    volumes: Option<usize>,
}

// 普通图片不是待解压文件，用 None 表示。
fn inspect_archive(archive: &Path, password: Option<&str>) -> Result<Option<ArchiveInfo>> {
    let output = Command::new("7z")
        .args(["l", "-slt", "-spd"])
        .arg(format!("-p{}", password.unwrap_or_default()))
        .arg("--")
        .arg(archive)
        .output()
        .context("执行7z命令失败，请确保已安装7-Zip")?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    if output.status.success() {
        let properties: Vec<_> = stdout
            .lines()
            .skip_while(|line| *line != "--")
            .skip(1)
            .take_while(|line| !line.is_empty() && *line != "----------")
            .collect();
        let rar_volumes = properties.contains(&"Multivolume = +")
            && properties
                .iter()
                .any(|line| matches!(*line, "Type = Rar" | "Type = Rar5"));
        let count = if rar_volumes {
            properties
                .iter()
                .find_map(|line| line.strip_prefix("Volumes = "))
                .context("7-Zip 未报告 RAR 分卷数")?
                .parse()
                .context("7-Zip 报告的 RAR 分卷数无效")?
        } else {
            1
        };
        return Ok(Some(ArchiveInfo {
            encrypted: stdout.lines().any(|line| line == "Encrypted = +"),
            volumes: Some(count),
        }));
    }

    let stderr = String::from_utf8_lossy(&output.stderr);
    if password.is_none() && stderr.contains("open encrypted archive") {
        return Ok(Some(ArchiveInfo {
            encrypted: true,
            volumes: None,
        }));
    }
    if is_image_file(archive) && stderr.contains("open the file as archive") {
        return Ok(None);
    }
    let details = stdout
        .lines()
        .find_map(|line| line.strip_prefix("ERROR = "))
        .unwrap_or_else(|| stderr.trim());
    Err(anyhow!("无法读取压缩包 {:?}: {}", archive, details))
}

fn test_password(archive: &Path, password: &str) -> Result<bool> {
    let status = Command::new("7z")
        .arg("t")
        .arg(format!("-p{}", password))
        .args(["-spd", "--"])
        .arg(archive)
        .stdin(Stdio::null())
        .status()
        .context("测试密码失败")?;
    Ok(status.success())
}

fn get_password(archive: &Path) -> Result<String> {
    println!("压缩包需要密码: {:?}", archive);
    print!("请输入解压密码（直接回车跳过）: ");
    io::stdout().flush()?;
    let mut password = String::new();
    io::stdin().read_line(&mut password)?;
    let password = password.trim_end_matches(['\r', '\n']);
    if password.is_empty() {
        return Err(anyhow!("已跳过压缩包: {:?}", archive));
    }
    Ok(password.to_owned())
}

fn extract_archive(archive: &Path, output_dir: &Path, password: Option<&str>) -> Result<()> {
    let mut output_arg = OsString::from("-o");
    output_arg.push(output_dir);
    let status = Command::new("7z")
        .args(["x", "-aou", "-spd"])
        .arg(format!("-p{}", password.unwrap_or_default()))
        .arg(output_arg)
        .arg("--")
        .arg(archive)
        .stdin(Stdio::null())
        .status()
        .context("解压命令执行失败")?;
    if !status.success() {
        return Err(anyhow!("解压失败 {:?}，退出状态: {}", archive, status));
    }
    Ok(())
}

fn publish_contents(output_dir: &Path, archive: &Path) -> Result<()> {
    let names = fs::read_dir(output_dir)?
        .map(|entry| entry.map(|entry| entry.file_name()))
        .collect::<io::Result<Vec<_>>>()?;
    let parent = archive.parent().context("压缩包没有所在目录")?;
    let mut destination = parent.to_path_buf();
    for name in &names {
        match fs::symlink_metadata(parent.join(name)) {
            Ok(_) => {
                let mut folder_name = archive
                    .file_name()
                    .context("压缩包没有文件名")?
                    .to_os_string();
                folder_name.push(".extracted");
                destination = create_available_dir(&parent.join(folder_name))?;
                println!("目标有同名条目，解压结果保存至: {:?}", destination);
                break;
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }

    for (index, name) in names.iter().enumerate() {
        let target = destination.join(name);
        if let Err(error) = fs::rename(output_dir.join(name), &target) {
            for earlier in &names[..index] {
                fs::rename(destination.join(earlier), output_dir.join(earlier))
                    .with_context(|| format!("还原解压结果失败: {:?}", earlier))?;
            }
            if destination != parent {
                fs::remove_dir(&destination)?;
            }
            return Err(error).with_context(|| format!("无法保存解压结果: {:?}", target));
        }
    }
    fs::remove_dir(output_dir)?;
    Ok(())
}

fn create_available_dir(base: &Path) -> Result<PathBuf> {
    for suffix in 0_u64.. {
        let mut path = base.as_os_str().to_os_string();
        if suffix != 0 {
            path.push(format!(".{suffix}"));
        }
        let path = PathBuf::from(path);
        match fs::create_dir(&path) {
            Ok(()) => return Ok(path),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error).with_context(|| format!("无法创建目录: {:?}", path)),
        }
    }
    unreachable!()
}
