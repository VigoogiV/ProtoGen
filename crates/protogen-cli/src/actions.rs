//! 三个动作的实现，CLI 和交互模式共用。
//!
//! 返回 `Result<输出文本, 错误文本>`，由调用方决定打印到 stdout 还是写日志面板。

use std::path::{Path, PathBuf};

use protogen_core::encode::{self, Encoding};
use protogen_core::gen::{comments_inject, render};
use protogen_core::model::Allocation;
use protogen_core::{options, parse};

/// 生成请求
pub struct GenRequest {
    pub proto: PathBuf,
    pub include: Vec<PathBuf>,
    pub out_dir: Option<PathBuf>,
    pub options_file: Option<PathBuf>,
    pub no_notes: bool,
    pub no_align: bool,
    pub encoding: Encoding,
    pub lossy: bool,
    pub timestamp: Option<String>,
}

impl GenRequest {
    pub fn new(proto: impl Into<PathBuf>) -> Self {
        GenRequest {
            proto: proto.into(),
            include: Vec::new(),
            out_dir: None,
            options_file: None,
            no_notes: false,
            no_align: false,
            encoding: Encoding::Gb2312,
            lossy: false,
            timestamp: None,
        }
    }
}

/// 生成 `.pb.c` / `.pb.h`，返回完整生成日志
pub fn run_gen(req: &GenRequest) -> Result<String, String> {
    let t_start = std::time::Instant::now();
    let proto = &req.proto;
    if !proto.exists() {
        return Err(format!("文件不存在: {}", proto.display()));
    }

    let mut log = String::new();
    let proto_size = std::fs::metadata(proto).map(|m| m.len()).unwrap_or(0);
    log.push_str(&format!(
        "[INFO] 输入: {} ({})\n",
        show_path(proto),
        human_size(proto_size)
    ));

    // 读取 .options
    let opts_path = req
        .options_file
        .clone()
        .unwrap_or_else(|| proto.with_extension("options"));
    let options_map = if opts_path.exists() {
        let map = parse::read_options_file(&opts_path).map_err(|e| e.to_string())?;
        log.push_str(&format!(
            "[INFO] options: {} ({} 项)\n",
            show_path(&opts_path),
            map.len()
        ));
        map
    } else {
        log.push_str("[WARN] 未找到 .options，bytes/string 字段会退化为 callback\n");
        Default::default()
    };

    // 解析
    let t_parse = std::time::Instant::now();
    let file = parse::parse_proto(proto, &req.include, &options_map).map_err(|e| e.to_string())?;
    let parse_ms = t_parse.elapsed().as_millis();

    let field_count: usize = file.messages.iter().map(|m| m.fields.len()).sum();
    log.push_str(&format!(
        "[INFO] 解析: {} message / {} enum / {} field ({} ms)\n",
        file.messages.len(),
        file.enums.len(),
        field_count,
        parse_ms
    ));

    // 检查是否有 bytes/string 字段因为没有 max_size 退化成了 callback
    let mut degraded: Vec<String> = Vec::new();
    for m in &file.messages {
        for f in &m.fields {
            if matches!(f.pbtype.as_str(), "BYTES" | "STRING")
                && f.allocation == Allocation::Callback
            {
                degraded.push(format!("{}.{}", m.name(), f.name));
            }
        }
    }
    if !degraded.is_empty() {
        log.push_str(&format!(
            "[WARN] {} 个 bytes/string 字段没有 max_size，已退化为 callback:\n",
            degraded.len()
        ));
        for k in degraded.iter().take(10) {
            log.push_str(&format!("[WARN]   - {}\n", k));
        }
        if degraded.len() > 10 {
            log.push_str(&format!("[WARN]   ...（共 {} 个）\n", degraded.len()));
        }
    }

    // 生成
    let gen_opts = render::GenOptions {
        no_timestamp: req.timestamp.is_none(),
        nanopb_version: "nanopb-0.3.9.1".to_string(),
        timestamp: req.timestamp.clone().unwrap_or_default(),
    };
    let mut out = render::generate(&file, &gen_opts);

    if !req.no_notes {
        out.header = comments_inject::inject(&file, &out.header);
    }
    if !req.no_align {
        out.header = comments_inject::format_enum(&out.header);
    }

    log.push_str(&format!(
        "[INFO] 生成 .pb.h: {} 行\n",
        out.header.lines().count()
    ));
    log.push_str(&format!(
        "[INFO] 生成 .pb.c: {} 行\n",
        out.source.lines().count()
    ));

    if req.no_notes {
        log.push_str("[INFO] 注释同步: 已关闭\n");
    } else {
        log.push_str(&format!(
            "[INFO] 注释同步: 已开启（{} 处）\n",
            count_notes(&file)
        ));
    }
    if req.no_align {
        log.push_str("[INFO] 枚举对齐: 已关闭\n");
    } else {
        log.push_str(&format!("[INFO] 枚举对齐: {} 个枚举\n", file.enums.len()));
    }

    // 写出
    let dir = req.out_dir.clone().unwrap_or_else(|| {
        proto
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| PathBuf::from("."))
    });
    std::fs::create_dir_all(&dir).map_err(|e| format!("创建输出目录失败: {}", e))?;

    let header_path = dir.join(format!("{}.pb.h", file.base_name));
    let source_path = dir.join(format!("{}.pb.c", file.base_name));
    write_encoded(&header_path, &out.header, req.encoding, req.lossy)?;
    write_encoded(&source_path, &out.source, req.encoding, req.lossy)?;

    log.push_str(&format!(
        "[INFO] 编码: {} / CRLF\n",
        match req.encoding {
            Encoding::Utf8 => "UTF-8",
            Encoding::Gb2312 => "GB2312",
        }
    ));
    log.push_str(&format!("[INFO] 输出: {}\n", show_path(&header_path)));
    log.push_str(&format!("[INFO] 输出: {}\n", show_path(&source_path)));
    log.push_str(&format!(
        "[INFO] 完成，耗时 {} ms\n",
        t_start.elapsed().as_millis()
    ));

    Ok(log)
}

/// 统计 proto 里带注释的字段与枚举值数量
fn count_notes(file: &protogen_core::model::ProtoFile) -> usize {
    let fields: usize = file
        .messages
        .iter()
        .map(|m| m.fields.iter().filter(|f| !f.comment.is_empty()).count())
        .sum();
    let values: usize = file
        .enums
        .iter()
        .map(|e| e.values.iter().filter(|v| !v.comment.is_empty()).count())
        .sum();
    fields + values
}

fn human_size(n: u64) -> String {
    if n < 1024 {
        format!("{} B", n)
    } else {
        format!("{:.1} KB", n as f64 / 1024.0)
    }
}

/// 只收集 `.options` 内容与日志，不写文件（`--stdout` 用）
pub fn collect_options(proto: &Path) -> Result<(String, String), String> {
    let t_start = std::time::Instant::now();
    if !proto.exists() {
        return Err(format!("文件不存在: {}", proto.display()));
    }

    let mut log = String::new();
    let proto_size = std::fs::metadata(proto).map(|m| m.len()).unwrap_or(0);
    log.push_str(&format!(
        "[INFO] 输入: {} ({})\n",
        show_path(proto),
        human_size(proto_size)
    ));

    let file = parse::parse_proto(proto, &[], &Default::default()).map_err(|e| e.to_string())?;
    let content = options::generate_options(&file);

    if content.is_empty() {
        log.push_str("[WARN] 该 .proto 中没有需要上限声明的字段（bytes/string 或 repeated）\n");
        return Ok((String::new(), log));
    }

    log.push_str(&format!(
        "[INFO] 扫描到 {} 个需要上限声明的字段（bytes/string 或 repeated）\n",
        content.lines().count()
    ));
    log.push_str(&format!(
        "[INFO] 完成，耗时 {} ms\n",
        t_start.elapsed().as_millis()
    ));
    Ok((content, log))
}

/// 生成 `.options` 模板
pub fn run_options(proto: &Path, out: Option<PathBuf>) -> Result<(String, bool), String> {
    let (content, mut log) = collect_options(proto)?;
    if content.is_empty() {
        return Ok((log, false));
    }

    let path = out.unwrap_or_else(|| proto.with_extension("options"));
    let existed = path.exists();
    let bytes = encode::encode(&encode::to_crlf(&content), Encoding::Gb2312, true)
        .map_err(|e| e.to_string())?;
    std::fs::write(&path, bytes).map_err(|e| format!("写入失败: {}", e))?;

    // 把"完成"行挪到最后，中间插入写文件的记录
    if let Some(pos) = log.rfind("[INFO] 完成，耗时") {
        let tail = log.split_off(pos);
        if existed {
            log.push_str(&format!("[INFO] 已覆盖: {}\n", show_path(&path)));
        } else {
            log.push_str(&format!("[INFO] 已生成: {}\n", show_path(&path)));
        }
        log.push_str(&tail);
    }
    Ok((log, true))
}

/// 校验 `.options`
pub fn run_check(proto: &Path, options_file: Option<PathBuf>) -> Result<(bool, String), String> {
    let t_start = std::time::Instant::now();
    if !proto.exists() {
        return Err(format!("文件不存在: {}", proto.display()));
    }
    let opts_path = options_file.unwrap_or_else(|| proto.with_extension("options"));
    if !opts_path.exists() {
        return Err(format!(
            "找不到 {}\n请先点「生成 .options」",
            opts_path.display()
        ));
    }

    let mut log = String::new();
    log.push_str(&format!("[INFO] 校验: {}\n", show_path(proto)));

    let map = parse::read_options_file(&opts_path).map_err(|e| e.to_string())?;
    log.push_str(&format!(
        "[INFO] options: {} ({} 项)\n",
        show_path(&opts_path),
        map.len()
    ));

    let file = parse::parse_proto(proto, &[], &map).map_err(|e| e.to_string())?;
    let report = options::check(&file, &map);

    let mut ok = report.is_ok();

    if report.missing.is_empty() && report.blank_value.is_empty() {
        log.push_str("[INFO] 校验通过：所有 bytes/string 字段都已定义 max_size\n");
    } else {
        if !report.missing.is_empty() {
            log.push_str(&format!(
                "[WARN] 缺少 max_size 定义（{} 项）:\n",
                report.missing.len()
            ));
            for k in &report.missing {
                log.push_str(&format!("  - {}\n", k));
            }
        }
        if !report.blank_value.is_empty() {
            log.push_str(&format!(
                "[WARN] max_size 未填值（{} 项）:\n",
                report.blank_value.len()
            ));
            for k in &report.blank_value {
                log.push_str(&format!("  - {}\n", k));
            }
        }
        ok = false;
    }
    if !report.extra.is_empty() {
        log.push_str(&format!(
            "[WARN] .options 中有 {} 项在 proto 里找不到对应字段:\n",
            report.extra.len()
        ));
        for k in report.extra.iter().take(10) {
            log.push_str(&format!("  - {}\n", k));
        }
        if report.extra.len() > 10 {
            log.push_str(&format!("  ...（共 {} 项）\n", report.extra.len()));
        }
    }
    if !report.no_max_count.is_empty() {
        log.push_str(&format!(
            "[WARN] {} 个 repeated 字段没有 max_count，会退化成 callback:\n",
            report.no_max_count.len()
        ));
        for k in report.no_max_count.iter().take(10) {
            log.push_str(&format!("  - {}\n", k));
        }
        if report.no_max_count.len() > 10 {
            log.push_str(&format!("  ...（共 {} 项）\n", report.no_max_count.len()));
        }
    }

    log.push_str(&format!(
        "[INFO] 完成，耗时 {} ms\n",
        t_start.elapsed().as_millis()
    ));
    Ok((ok, log))
}

fn write_encoded(
    path: &Path,
    text: &str,
    enc: Encoding,
    lossy: bool,
) -> Result<(), String> {
    let with_crlf = encode::to_crlf(text);
    let bytes = encode::encode(&with_crlf, enc, lossy).map_err(|e| e.to_string())?;
    std::fs::write(path, bytes).map_err(|e| format!("写入 {} 失败: {}", path.display(), e))?;
    Ok(())
}

/// 显示路径时统一分隔符，避免出现 `a/b\\c` 这种混用
pub fn show_path(p: &Path) -> String {
    let s = p.display().to_string();
    #[cfg(windows)]
    {
        s.replace('/', "\\")
    }
    #[cfg(not(windows))]
    {
        s
    }
}

/// 解析编码参数
pub fn parse_encoding(s: &str) -> Encoding {
    match s.to_ascii_lowercase().as_str() {
        "utf8" | "utf-8" => Encoding::Utf8,
        _ => Encoding::Gb2312,
    }
}
