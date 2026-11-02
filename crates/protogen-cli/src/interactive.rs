//! 交互模式：双击 exe 时的入口。
//!
//! 旧工具是 WinForms，双击就出界面。这里是纯 CLI，双击如果没有参数，
//! 控制台窗口会瞬间关闭——所以无参数时进入这个菜单式交互，让双击可用。

use std::io::{self, BufRead, Write};
use std::path::PathBuf;
use std::process::ExitCode;

use protogen_core::encode::Encoding;

use crate::actions::{self, GenRequest};

/// 启动交互模式
pub fn run() -> ExitCode {
    print_banner();

    // 双击启动时直接弹选择框，省掉一次回车；取消则回落到手动输入。
    // 脚本/测试场景可设 PROTOGEN_NO_DIALOG=1 跳过。
    let dialog_allowed =
        crate::filedialog::HAS_DIALOG && std::env::var_os("PROTOGEN_NO_DIALOG").is_none();
    let mut pending: Option<PathBuf> = if dialog_allowed {
        crate::filedialog::pick_proto_file()
    } else {
        None
    };

    loop {
        let proto = match pending.take() {
            Some(p) => p,
            None => match pick_file(dialog_allowed) {
                Some(p) => p,
                None => return ExitCode::SUCCESS,
            },
        };

        if !proto.exists() {
            println!();
            println!("  文件不存在: {}", proto.display());
            pause();
            continue;
        }

        println!();
        println!("  已选择: {}", proto.display());

        loop {
            println!();
            println!("  ── 请选择操作 ────────────────────────");
            println!("    1) 生成 .pb.c / .pb.h（含注释同步）");
            println!("    2) 生成 .options 模板");
            println!("    3) 校验 .options 是否完整");
            println!("    4) 换个 .proto 文件");
            println!("    0) 退出");
            println!("  ──────────────────────────────────────");

            let choice = match prompt("  输入序号: ") {
                Some(v) => v,
                None => return ExitCode::SUCCESS,
            };

            match choice.as_str() {
                "1" => {
                    let mut req = GenRequest::new(proto.clone());
                    // 交互模式默认输出到 proto 同目录，与旧工具一致
                    req.encoding = Encoding::Gb2312;

                    // 若同名 .options 存在，顺便提示
                    let opt = proto.with_extension("options");
                    println!();
                    println!("  正在生成...");
                    match actions::run_gen(&req) {
                        Ok(log) => print_block(&log),
                        Err(e) => print_error(&e),
                    }
                    if !opt.exists() {
                        println!("  提示: 没有 .options，bytes/string 字段会退化成 callback。");
                        println!("        可以先用选项 2 生成模板再重新生成。");
                    }
                    pause();
                }
                "2" => {
                    println!();
                    let target = proto.with_extension("options");
                    if target.exists() {
                        let ans = prompt("  .options 已存在，覆盖? [y/N]: ")
                            .unwrap_or_default()
                            .to_ascii_lowercase();
                        if ans != "y" && ans != "yes" {
                            println!("  已取消");
                            pause();
                            continue;
                        }
                    }
                    match actions::run_options(&proto, None) {
                        Ok((log, _)) => print_block(&log),
                        Err(e) => print_error(&e),
                    }
                    pause();
                }
                "3" => {
                    println!();
                    match actions::run_check(&proto, None) {
                        Ok((ok, log)) => {
                            print_block(&log);
                            if ok {
                                println!("  [通过]");
                            } else {
                                println!("  [未通过] 请补齐 .options 后重试");
                            }
                        }
                        Err(e) => print_error(&e),
                    }
                    pause();
                }
                "4" => break,
                "0" | "q" | "quit" | "exit" => return ExitCode::SUCCESS,
                other => {
                    println!("  无效输入: {}", other);
                }
            }
        }
    }
}

fn print_banner() {
    println!();
    println!("  ══════════════════════════════════════════════════");
    println!("   protogen  ·  proto → nanopb C 代码");
    println!("   注释同步 / 枚举对齐 / .options 生成与校验");
    println!("  ══════════════════════════════════════════════════");
    println!();
    if crate::filedialog::HAS_DIALOG && std::env::var_os("PROTOGEN_NO_DIALOG").is_none() {
        println!("  即将打开文件选择框；取消后也可以手动输入路径。");
    } else {
        println!("  输入或粘贴 .proto 的完整路径，输入 q 退出。");
    }
    println!();
}

/// 让用户选一个 proto 文件，返回 None 表示退出
fn pick_file(dialog_allowed: bool) -> Option<PathBuf> {
    loop {
        let label = if dialog_allowed {
            "  .proto 文件（回车=选择框）: "
        } else {
            "  .proto 文件路径: "
        };
        let raw = prompt(label)?;
        let cleaned = clean_path(&raw);

        // 空输入：弹系统文件选择框
        // （Windows conhost 不支持拖放文件，这是双击用户唯一可靠的方式）
        if cleaned.is_empty() {
            if dialog_allowed {
                if let Some(path) = crate::filedialog::pick_proto_file() {
                    return Some(path);
                }
            }
            // 用户取消了对话框，回到提示
            continue;
        }

        if matches!(cleaned.to_ascii_lowercase().as_str(), "q" | "quit" | "exit") {
            return None;
        }

        let path = PathBuf::from(&cleaned);
        if !path.exists() {
            println!("  找不到该文件，请检查路径。");
            continue;
        }
        if path.extension().and_then(|e| e.to_str()) != Some("proto") {
            println!("  警告：这个文件不是 .proto 后缀，仍然继续。");
        }
        return Some(path);
    }
}

/// 去掉拖拽带来的引号、首尾空白
fn clean_path(s: &str) -> String {
    let t = s.trim();
    let t = t.strip_prefix('"').unwrap_or(t);
    let t = t.strip_suffix('"').unwrap_or(t);
    let t = t.strip_prefix('\'').unwrap_or(t);
    let t = t.strip_suffix('\'').unwrap_or(t);
    t.trim().to_string()
}

/// 打印一段带缩进的文本块
fn print_block(text: &str) {
    for line in text.lines() {
        println!("  {}", line);
    }
}

fn print_error(e: &str) {
    println!("  [错误] {}", e);
    for line in e.lines().skip(1) {
        println!("         {}", line);
    }
}

/// 等待用户按回车
fn pause() {
    print!("\n  按回车继续...");
    let _ = io::stdout().flush();
    let mut buf = String::new();
    let _ = io::stdin().lock().read_line(&mut buf);
}

/// 显示提示并读一行；返回 None 表示输入已结束（EOF）
fn prompt(label: &str) -> Option<String> {
    print!("{}", label);
    let _ = io::stdout().flush();

    let mut buf = String::new();
    match io::stdin().lock().read_line(&mut buf) {
        Ok(0) => {
            // stdin 关闭（例如被重定向且没有内容），安静退出
            println!();
            None
        }
        Ok(_) => Some(buf.trim().to_string()),
        Err(_) => None,
    }
}
