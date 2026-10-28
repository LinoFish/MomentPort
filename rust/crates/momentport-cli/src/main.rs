//! momentport-cli：MomentPort v1.1 Rust 核心的命令行入口（仅标准库）。
//!
//! 用法：
//!   momentport-cli <输入文件夹> <输出文件夹> [--gap 秒] [--dry-run|--list]
//!                    [--key-index N] [--key-indexes 0,2,1] [--copy-singles]
//!   momentport-cli --input <输入> --output <输出> [同上选项]
//!   momentport-cli <输入文件夹> --list-json [--gap 秒] [--key-indexes …]
//!
//! 行为口径：
//! - 输入文件夹必填；输出文件夹在转换时必填，--list-json 预览时可省略；
//!   缺失时打印用法并以 exit 2 退出；
//! - --gap 默认 2.0 秒：平铺文件夹里相邻两张间隔 > gap 切分，≤ gap 归一组；
//! - --dry-run / --list 只列分组不写文件，不创建输出文件夹；
//! - --list-json 只列分组不写文件，输出机器可读 JSON（见 preview_json，
//!   供 SwiftUI 界面解析，不依赖上面的人类可读清单）；
//! - --key-index 默认 0（第一帧为关键帧；越界时按组内最后一帧计）；
//! - --key-indexes 按组顺序给每组不同的关键帧（逗号分隔，缺省的组用
//!   --key-index 的值；越界时按组内最后一帧计）；
//! - --copy-singles 可选：把单张原样拷到 Singles/（默认只记录，不写 BurstUUID）；
//! - 输出：每组一个 Burst_01/、Burst_02/…子文件夹 + BurstManifest.json，
//!   顶层写 MomentPortReport.json（汇总组、单张、live 配对与未处理原因）；
//! - 转换后回读校验失败的帧记入 skipped，不计入成功帧数；
//! - 原图文件夹原封不动；本 CLI 不导入相册、不声称相册已成组。

use momentport_core::group::Grouping;
use momentport_core::json::quoted;
use momentport_core::scan::ScanResult;
use momentport_core::timeutil::format_iso_ms;
use momentport_core::{convert_all, group_photos, pair_for_photo, scan_dir};
use std::path::PathBuf;
use std::process::ExitCode;

const USAGE: &str = "\
用法：momentport-cli <输入文件夹> <输出文件夹> [选项]
   或：momentport-cli --input <输入文件夹> --output <输出文件夹> [选项]
   或：momentport-cli <输入文件夹> --list-json [选项]

选项：
  --gap 秒          平铺分组间隔阈值，默认 2.0（间隔 > gap 切分）
  --dry-run, --list 只列分组，不写文件
  --list-json       只列分组不写文件，输出 JSON 预览（供界面解析）
  --key-index N     关键帧索引，默认 0
  --key-indexes A,B 按组顺序的关键帧索引（逗号分隔，缺省组用 --key-index）
  --copy-singles    把单张原样拷到 Singles/（默认只记录，不写 BurstUUID）
  -h, --help        显示本帮助
";

struct Args {
    input: Option<PathBuf>,
    output: Option<PathBuf>,
    gap: f64,
    dry_run: bool,
    list_json: bool,
    key_index: usize,
    key_indexes: Option<Vec<usize>>,
    copy_singles: bool,
}

/// 机器可读分组预览。键名与 Swift 端 RustPreview Codable 逐一对齐，改这里
/// 必须同步改 RustBackend.swift，否则界面解析会失败（见 README 字段对照表）。
fn preview_json(
    grouping: &Grouping,
    scan: &ScanResult,
    gap: f64,
) -> String {
    let mut s = String::new();
    s.push_str("{\n");
    s.push_str(&format!("  \"gap\": {},\n", gap));
    s.push_str("  \"groups\": [");
    if grouping.groups.is_empty() {
        s.push_str("],\n");
    } else {
        s.push('\n');
        for (gi, g) in grouping.groups.iter().enumerate() {
            s.push_str("    {\n");
            s.push_str(&format!(
                "      \"keyIndex\": {},\n",
                g.key_index
            ));
            s.push_str("      \"photos\": [");
            if g.photos.is_empty() {
                s.push_str("],\n");
            } else {
                s.push('\n');
                for (pi, p) in g.photos.iter().enumerate() {
                    let live = pair_for_photo(&scan.live_pairs, p)
                        .map(|pr| quoted(&pr.video_file_name))
                        .unwrap_or_else(|| "null".to_string());
                    s.push_str("        {\n");
                    s.push_str(&format!("          \"captureISO\": {},\n", quoted(&format_iso_ms(p.capture_ms))));
                    s.push_str(&format!("          \"captureMs\": {},\n", p.capture_ms));
                    s.push_str(&format!("          \"dirKey\": {},\n", quoted(&p.dir_key)));
                    s.push_str(&format!("          \"fileName\": {},\n", quoted(&p.file_name)));
                    s.push_str(&format!("          \"hasExifDate\": {},\n", p.has_exif_date));
                    s.push_str(&format!("          \"liveVideoFileName\": {}\n", live));
                    s.push_str(if pi + 1 == g.photos.len() {
                        "        }\n"
                    } else {
                        "        },\n"
                    });
                }
                s.push_str("      ],\n");
            }
            s.push_str(&format!(
                "      \"sourceDescription\": {}\n",
                quoted(&g.source_description)
            ));
            s.push_str(if gi + 1 == grouping.groups.len() {
                "    }\n"
            } else {
                "    },\n"
            });
        }
        s.push_str("  ],\n");
    }
    s.push_str("  \"livePairs\": [");
    if scan.live_pairs.is_empty() {
        s.push_str("],\n");
    } else {
        s.push('\n');
        for (i, pr) in scan.live_pairs.iter().enumerate() {
            s.push_str("    {\n");
            s.push_str(&format!("      \"photoDirKey\": {},\n", quoted(&pr.photo_dir_key)));
            s.push_str(&format!("      \"photoFileName\": {},\n", quoted(&pr.photo_file_name)));
            s.push_str(&format!("      \"videoFileName\": {}\n", quoted(&pr.video_file_name)));
            s.push_str(if i + 1 == scan.live_pairs.len() {
                "    }\n"
            } else {
                "    },\n"
            });
        }
        s.push_str("  ],\n");
    }
    s.push_str("  \"singles\": [");
    if grouping.singles.is_empty() {
        s.push_str("],\n");
    } else {
        s.push('\n');
        for (i, p) in grouping.singles.iter().enumerate() {
            let live = pair_for_photo(&scan.live_pairs, p)
                .map(|pr| quoted(&pr.video_file_name))
                .unwrap_or_else(|| "null".to_string());
            s.push_str("    {\n");
            s.push_str(&format!("      \"captureISO\": {},\n", quoted(&format_iso_ms(p.capture_ms))));
            s.push_str(&format!("      \"captureMs\": {},\n", p.capture_ms));
            s.push_str(&format!("      \"dirKey\": {},\n", quoted(&p.dir_key)));
            s.push_str(&format!("      \"fileName\": {},\n", quoted(&p.file_name)));
            s.push_str(&format!("      \"hasExifDate\": {},\n", p.has_exif_date));
            s.push_str(&format!("      \"liveVideoFileName\": {}\n", live));
            s.push_str(if i + 1 == grouping.singles.len() {
                "    }\n"
            } else {
                "    },\n"
            });
        }
        s.push_str("  ],\n");
    }
    s.push_str("  \"unprocessed\": [");
    if scan.unprocessed.is_empty() {
        s.push_str("]\n");
    } else {
        s.push('\n');
        for (i, u) in scan.unprocessed.iter().enumerate() {
            s.push_str("    {\n");
            s.push_str(&format!("      \"dirKey\": {},\n", quoted(&u.dir_key)));
            s.push_str(&format!("      \"fileName\": {},\n", quoted(&u.file_name)));
            s.push_str(&format!("      \"reason\": {}\n", quoted(&u.reason)));
            s.push_str(if i + 1 == scan.unprocessed.len() {
                "    }\n"
            } else {
                "    },\n"
            });
        }
        s.push_str("  ]\n");
    }
    s.push_str("}\n");
    s
}

fn parse_args(argv: &[String]) -> Result<Args, String> {
    let mut args = Args {
        input: None,
        output: None,
        gap: 2.0,
        dry_run: false,
        list_json: false,
        key_index: 0,
        key_indexes: None,
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
            "--list-json" => args.list_json = true,
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
            "--key-indexes" => {
                let v = take_value(&mut i, inline_val)?;
                let mut list = Vec::new();
                for part in v.split(',') {
                    let part = part.trim();
                    if part.is_empty() {
                        continue;
                    }
                    list.push(
                        part.parse::<usize>().map_err(|_| {
                            format!("--key-indexes 取值不是逗号分隔的非负整数：{}", v)
                        })?,
                    );
                }
                args.key_indexes = Some(list);
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
    // --list-json 是纯预览，不写文件，输出文件夹可省略。
    let output = match args.output {
        Some(p) => Some(p),
        None if args.list_json => None,
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
    let mut grouping = group_photos(&scan.photos, args.gap, args.key_index);
    // 每组不同的关键帧：按组顺序覆盖，缺省的组保留 --key-index 的值。
    // 越界不报错，key_photo() 在使用时钳制到最后一帧（与旧口径一致）。
    if let Some(list) = &args.key_indexes {
        for (i, g) in grouping.groups.iter_mut().enumerate() {
            if let Some(k) = list.get(i) {
                g.key_index = *k;
            }
        }
    }

    if args.list_json {
        print!("{}", preview_json(&grouping, &scan, args.gap));
        return ExitCode::SUCCESS;
    }
    let output = match output {
        Some(p) => p,
        None => {
            eprintln!("错误：输出文件夹必填\n{}", USAGE);
            return ExitCode::from(2);
        }
    };

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

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(v: &[&str]) -> Args {
        parse_args(&v.iter().map(|s| s.to_string()).collect::<Vec<_>>()).unwrap()
    }

    #[test]
    fn list_json_does_not_require_output() {
        let a = parse(&["/tmp/in", "--list-json"]);
        assert!(a.list_json);
        assert!(a.output.is_none());
    }

    #[test]
    fn key_indexes_parsed_in_group_order() {
        let a = parse(&["/tmp/in", "/tmp/out", "--key-indexes", "0,2,1"]);
        assert_eq!(a.key_indexes, Some(vec![0, 2, 1]));
    }

    #[test]
    fn key_indexes_bad_value_is_error() {
        let r = parse_args(&["/tmp/in".to_string(), "--key-indexes".to_string(), "0,x".to_string()]);
        assert!(r.is_err());
    }
}
