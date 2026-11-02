//! protogen CLI：独立运行的 proto → nanopb C 代码工具。
//!
//! 三种启动方式：
//! 1. 双击 / 无参数 → 进入交互菜单（不闪退）
//! 2. 把一个 .proto 拖到 exe 上 → 直接生成后暂停
//! 3. 正常命令行 → 子命令模式

mod actions;
mod filedialog;
mod interactive;

use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};

use actions::GenRequest;

#[derive(Parser)]
#[command(
    name = "protogen",
    version,
    about = "根据 .proto 生成 nanopb 的 .pb.c/.pb.h，并同步注释、生成 .options"
)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// 生成 .pb.c 与 .pb.h
    Gen {
        /// 输入的 .proto 文件
        proto: PathBuf,
        /// 额外的 include 目录
        #[arg(short = 'I', long = "include")]
        include: Vec<PathBuf>,
        /// 输出目录，默认为 proto 所在目录
        #[arg(short = 'o', long = "out")]
        out_dir: Option<PathBuf>,
        /// 指定 .options 文件，默认找同名 .options
        #[arg(long = "options")]
        options_file: Option<PathBuf>,
        /// 不注入注释
        #[arg(long)]
        no_notes: bool,
        /// 不做枚举值对齐
        #[arg(long)]
        no_align: bool,
        /// 输出编码：gb2312（默认，兼容旧工具）或 utf8
        #[arg(long, default_value = "gb2312")]
        encoding: String,
        /// GB2312 无法表示字符时降级为 '?' 而不是报错
        #[arg(long)]
        lossy: bool,
        /// 生成时间戳（默认不写，保证可复现）
        #[arg(long)]
        timestamp: Option<String>,
    },
    /// 生成 .options 模板
    Options {
        proto: PathBuf,
        /// 输出路径，默认为 proto 同名 .options
        #[arg(short = 'o', long = "out")]
        out: Option<PathBuf>,
        /// 只打印到 stdout，不写文件
        #[arg(long)]
        stdout: bool,
    },
    /// 校验 .options 是否覆盖所有 bytes/string 字段
    Check {
        proto: PathBuf,
        /// 指定 .options 文件，默认找同名
        #[arg(long = "options")]
        options_file: Option<PathBuf>,
    },
}

fn main() -> ExitCode {
    enable_utf8_console();

    let args: Vec<String> = std::env::args().skip(1).collect();

    // 1. 无参数（双击）：进交互菜单，避免窗口一闪就关
    if args.is_empty() {
        // 即使内部出错也不让窗口直接消失
        return match std::panic::catch_unwind(interactive::run) {
            Ok(code) => code,
            Err(_) => {
                println!();
                println!("  程序内部错误，详情见上面的信息。");
                wait_for_enter("  按回车关闭窗口...");
                ExitCode::FAILURE
            }
        };
    }

    // 2. 单个参数且是 .proto（把文件拖到 exe 图标上）：直接生成后暂停
    if args.len() == 1 && !args[0].starts_with('-') {
        let candidate = PathBuf::from(&args[0]);
        if candidate.exists()
            && candidate.extension().and_then(|e| e.to_str()) == Some("proto")
        {
            return run_drag_and_drop(&candidate);
        }
    }

    // 3. 正常命令行
    let cli = Cli::parse();
    match run(cli) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("错误: {}", e);
            ExitCode::FAILURE
        }
    }
}

/// 等待用户按回车（双击场景用，避免窗口瞬间关闭）
fn wait_for_enter(label: &str) {
    print!("{}", label);
    let _ = std::io::stdout().flush();
    let mut buf = String::new();
    let _ = std::io::stdin().read_line(&mut buf);
}

/// 把 .proto 拖到 exe 上时走这里：生成完停住，让用户看到结果
fn run_drag_and_drop(proto: &std::path::Path) -> ExitCode {
    println!("  protogen  ·  {}", proto.display());
    println!();

    let req = GenRequest::new(proto);
    let code = match actions::run_gen(&req) {
        Ok(log) => {
            for line in log.lines() {
                println!("  {}", line);
            }
            ExitCode::SUCCESS
        }
        Err(e) => {
            println!("  [错误] {}", e);
            ExitCode::FAILURE
        }
    };

    println!();
    wait_for_enter("  按回车关闭窗口...");
    code
}

fn run(cli: Cli) -> Result<ExitCode, Box<dyn std::error::Error>> {
    match cli.cmd {
        Cmd::Gen {
            proto,
            include,
            out_dir,
            options_file,
            no_notes,
            no_align,
            encoding,
            lossy,
            timestamp,
        } => {
            let mut req = GenRequest::new(proto);
            req.include = include;
            req.out_dir = out_dir;
            req.options_file = options_file;
            req.no_notes = no_notes;
            req.no_align = no_align;
            req.encoding = actions::parse_encoding(&encoding);
            req.lossy = lossy;
            req.timestamp = timestamp;

            match actions::run_gen(&req) {
                Ok(log) => {
                    print!("{}", log);
                    Ok(ExitCode::SUCCESS)
                }
                Err(e) => {
                    eprintln!("错误: {}", e);
                    Ok(ExitCode::FAILURE)
                }
            }
        }

        Cmd::Options { proto, out, stdout } => {
            if stdout {
                // 内容走 stdout、日志走 stderr，调用方可以干净地分离两者
                return match actions::collect_options(&proto) {
                    Ok((content, log)) => {
                        eprint!("{}", log);
                        if !content.is_empty() {
                            print!("{}", content);
                        }
                        Ok(ExitCode::SUCCESS)
                    }
                    Err(e) => {
                        eprintln!("错误: {}", e);
                        Ok(ExitCode::FAILURE)
                    }
                };
            }
            match actions::run_options(&proto, out) {
                Ok((log, _)) => {
                    print!("{}", log);
                    Ok(ExitCode::SUCCESS)
                }
                Err(e) => {
                    eprintln!("错误: {}", e);
                    Ok(ExitCode::FAILURE)
                }
            }
        }

        Cmd::Check {
            proto,
            options_file,
        } => match actions::run_check(&proto, options_file) {
            Ok((ok, log)) => {
                print!("{}", log);
                Ok(if ok {
                    ExitCode::SUCCESS
                } else {
                    ExitCode::FAILURE
                })
            }
            Err(e) => {
                eprintln!("错误: {}", e);
                Ok(ExitCode::FAILURE)
            }
        },
    }
}

/// Windows 控制台默认是 GBK 代码页，Rust 输出 UTF-8 会乱码，这里切到 65001
#[cfg(windows)]
fn enable_utf8_console() {
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn SetConsoleOutputCP(code_page: u32) -> i32;
        fn SetConsoleCP(code_page: u32) -> i32;
    }
    unsafe {
        SetConsoleOutputCP(65001);
        SetConsoleCP(65001);
    }
}

#[cfg(not(windows))]
fn enable_utf8_console() {}
