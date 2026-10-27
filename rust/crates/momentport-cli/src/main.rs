//! momentport-cli：MomentPort v1.1 Rust 核心的命令行入口（仅标准库）。
//!
//! 用法：
//!   momentport-cli <输入文件夹> <输出文件夹> [--gap 秒] [--dry-run|--list]
//!                    [--key-index N] [--copy-singles]
//!   momentport-cli --input <输入> --output <输出> [同上选项]
//!
//! 行为口径：
//! - 输入/输出文件夹必填；缺失时打印用法并以 exit 2 退出；
//! - --gap 默认 2.0 秒：平铺文件夹里相邻两张间隔 > gap 切分，≤ gap 归一组；
//! - --dry-run / --list 只列分组不写文件，不创建输出文件夹；
//! - --key-index 默认 0（第一帧为关键帧；越界时按组内最后一帧计）；
//! - --copy-singles 可选：把单张原样拷到 Singles/（默认只记录，不写 BurstUUID）；
//! - 输出：每组一个 Burst_01/、Burst_02/…子文件夹 + BurstManifest.json，
//!   顶层写 MomentPortReport.json（汇总组、单张、live 配对与未处理原因）；
//! - 转换后回读校验失败的帧记入 skipped，不计入成功帧数；
//! - 原图文件夹原封不动；本 CLI 不导入相册、不声称相册已成组。

use momentport_core::{convert_all, group_photos, pair_for_photo, scan_dir};
use std::path::PathBuf;
use std::process::ExitCode;

const USAGE: &str = "\
用法：momentport-cli <输入文件夹> <输出文件夹> [选项]
   或：momentport-cli --input <输入文件夹> --output <输出文件夹> [选项]

选项：
  --gap 秒          平铺分组间隔阈值，默认 2.0（间隔 > gap 切分）
  --dry-run, --list 只列分组，不写文件
  --key-index N     关键帧索引，默认 0
  --copy-singles    把单张原样拷到 Singles/（默认只记录，不写 BurstUUID）
  -h, --help        显示本帮助
";

struct Args {
    input: Option<PathBuf>,
    output: Option<PathBuf>,
    gap: f64,
    dry_run: bool,
    key_index: usize,
    copy_singles: bool,
}

fn parse_args(argv: &[String]) -> Result<Args, String> {
    let mut args = Args {
        input: None,
        output: None,
        gap: 2.0,
        dry_run: false,
        key_index: 0,
        copy_singles: false,
    };
    let mut positional: Vec<String> = Vec::new();
    let mut i = 0;
    while i < argv.len() {
        let a = &argv[i];
        let (flag, inline_val): (&str, Option<String>) = match a.split_once('=') {
            Some((f, v)) if f.starts_with("--") => (f, Some(v.to_string())),
            _ => (a.as_str(), None),
        };
        let take_value = |i: &mut usize, inline: Option<String>| -> Result<String, String> {
            if let Some(v) = inline {
                return Ok(v);
            }
            *i += 1;
            argv.get(*i)
                .cloned()
                .ok_or_else(|| "选项缺少取值".to_string())
        };
        match flag {
            "-h" | "--help" => return Err("__help__".to_string()),
            "--dry-run" | "--list" => args.dry_run = true,
            "--copy-singles" => args.copy_singles = true,
            "--gap" => {
                let v = take_value(&mut i, inline_val)?;
                args.gap = v.parse::<f64>().map_err(|_| format!("--gap 取值不是数字：{}", v))?;
                if !(args.gap >= 0.0) {
                    return Err("--gap 不能为负数".to_string());
                }
            }
            "--key-index" => {
                let v = take_value(&mut i, inline_val)?;
                args.key_index = v
                    .parse::<usize>()
                    .map_err(|_| format!("--key-index 取值不是非负整数：{}", v))?;
            }
            "--input" => {
                let v = take_value(&mut i, inline_val)?;
                args.input = Some(PathBuf::from(v));
            }
            "--output" => {
                let v = take_value(&mut i, inline_val)?;
                args.output = Some(PathBuf::from(v));
            }
            _ if flag.starts_with('-') => {
                return Err(format!("未知选项：{}", flag));
            }
            _ => positional.push(a.clone()),
        }
        i += 1;
    }
    if args.input.is_none() && !positional.is_empty() {
        args.input = Some(PathBuf::from(positional.remove(0)));
    }
    if args.output.is_none() && !positional.is_empty() {
        args.output = Some(PathBuf::from(positional.remove(0)));
    }
    if !positional.is_empty() {
        return Err(format!("多余的位置参数：{}", positional.join(" ")));
    }
    Ok(args)
}

fn dir_display(key: &str) -> String {
    if key.is_empty() {
        "(顶层)".to_string()
    } else {
        key.to_string()
    }
}

fn main() -> ExitCode {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let args = match parse_args(&argv) {
        Ok(a) => a,
        Err(e) if e == "__help__" => {
            print!("{}", USAGE);
            return ExitCode::SUCCESS;
        }
        Err(e) => {
            eprintln!("错误：{}\n{}", e, USAGE);
            return ExitCode::from(2);
        }
    };
    let input = match args.input {
        Some(p) => p,
        None => {
            eprintln!("错误：输入文件夹必填\n{}", USAGE);
            return ExitCode::from(2);
        }
    };
    let output = match args.output {
        Some(p) => p,
        None => {
            eprintln!("错误：输出文件夹必填\n{}", USAGE);
            return ExitCode::from(2);
        }
    };

    let scan = match scan_dir(&input) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("错误：{}", e);
            return ExitCode::from(1);
        }
    };
    let grouping = group_photos(&scan.photos, args.gap, args.key_index);

    // —— 分组清单（dry-run 与正式转换都打印）——
    println!("输入：{}", input.display());
    println!("输出：{}", output.display());
    println!(
        "扫描到 JPEG {} 张，live 配对 {} 对，未处理 {} 项",
        scan.photos.len(),
        scan.live_pairs.len(),
        scan.unprocessed.len()
    );
    println!("连拍组 {} 组（gap={} 秒，key-index={}）：", grouping.groups.len(), args.gap, args.key_index);
    for (i, g) in grouping.groups.iter().enumerate() {
        println!(
            "  Burst_{:02}  {} 帧  {}  关键帧：{}",
            i + 1,
            g.photos.len(),
            g.source_description,
            g.key_photo()
                .map(|p| p.file_name.clone())
                .unwrap_or_default()
        );
        for p in &g.photos {
            let pair = pair_for_photo(&scan.live_pairs, p)
                .map(|pr| format!(" + live 配对 {}", pr.video_file_name))
                .unwrap_or_default();
            println!(
                "    - {}  {}{}",
                p.file_name,
                if p.has_exif_date { "EXIF时间" } else { "mtime兜底" },
                pair
            );
        }
    }
    println!("单张 {} 张（不是连拍，不写 BurstUUID）：", grouping.singles.len());
    for p in &grouping.singles {
        let pair = pair_for_photo(&scan.live_pairs, p)
            .map(|pr| format!(" + live 配对 {}", pr.video_file_name))
            .unwrap_or_default();
        println!("  - {}（{}）{}", p.file_name, dir_display(&p.dir_key), pair);
    }
    println!("未处理 {} 项：", scan.unprocessed.len());
    for u in &scan.unprocessed {
        println!("  - {}（{}）：{}", u.file_name, dir_display(&u.dir_key), u.reason);
    }

    if args.dry_run {
        println!("[dry-run] 只列分组，未写任何文件。");
        return ExitCode::SUCCESS;
    }

    match convert_all(&output, &grouping, &scan.live_pairs, &scan.unprocessed, args.copy_singles) {
        Ok(outcome) => {
            for g in &outcome.groups {
                println!(
                    "已转换 {}：{} 帧成功，{} 帧跳过",
                    g.folder_name,
                    g.output_files.len(),
                    g.skipped.len()
                );
                for s in &g.skipped {
                    println!("  跳过 {}：{}", s.file_name, s.reason);
                }
            }
            if outcome.singles_copied {
                println!("单张已原样复制到 Singles/（未写 BurstUUID）。");
            }
            println!("报告：{}", outcome.report_path.display());
            println!("注意：这只证明文件层 BurstUUID 写对了；相册是否成组，须在 macOS 上导入后用 PhotoKit 回查 burstIdentifier。");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("转换失败：{}", e);
            ExitCode::from(1)
        }
    }
}
