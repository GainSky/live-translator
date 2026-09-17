//! M1 冒烟测试工具（不启动 GUI，直接跑核心流水线）
//!
//! 用法（在 src-tauri 下，模型目录默认 `models/` 或环境变量 LIVE_TRANSLATOR_MODELS_DIR）：
//!   cargo run --features audio-pipewire --bin smoke -- list
//!   cargo run --features audio-pipewire --bin smoke -- decode <wav路径> [模型目录]
//!   cargo run --features audio-pipewire --bin smoke -- capture [模型目录] [设备名子串] [秒数]

use std::path::PathBuf;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use cpal::traits::StreamTrait;
use live_translator_lib::asr::sense_voice::SenseVoiceEngine;
use live_translator_lib::asr::{vad::Segmenter, vad::VadParams, ASREngine};
use live_translator_lib::audio;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("list") => cmd_list(),
        Some("probe") => cmd_probe(),
        Some("translate") => {
            // smoke translate "文本" [模型目录] [目标语言]
            if let Some(text) = args.get(2).cloned() {
                cmd_translate(&text, args.get(3).map(String::as_str), args.get(4).map(String::as_str));
            } else {
                usage();
            }
        }
        Some("decode") => {
            if let Some(wav) = args.get(2) {
                cmd_decode(wav, args.get(3).map(String::as_str));
            } else {
                usage();
            }
        }
        Some("capture") => cmd_capture(
            args.get(2).map(String::as_str),
            args.get(3).map(String::as_str),
            args.get(4).and_then(|s| s.parse::<u64>().ok()).unwrap_or(30),
        ),
        _ => usage(),
    }
}

fn usage() {
    eprintln!("用法:");
    eprintln!("  smoke list                                   # 列出音频设备");
    eprintln!("  smoke probe                                  # 信号测试（并行探测各源峰值并排序）");
    eprintln!("  smoke translate \"文本\" [模型目录] [目标语言]    # 内置引擎翻译测速");
    eprintln!("  smoke decode <wav> [模型目录]                 # WAV 离线解码（VAD→ASR）");
    eprintln!("  smoke capture [模型目录] [设备名子串] [秒数]   # 实时采集转写");
    std::process::exit(2);
}

fn cmd_translate(text: &str, model_dir: Option<&str>, target: Option<&str>) {
    use live_translator_lib::translate::local_engine::{LocalEngine, DEFAULT_MODEL};
    let root = model_root(model_dir);
    let target = target.unwrap_or("zh-TW");
    eprintln!("模型目录: {}（首次加载含模型读盘）", root.display());
    let t0 = Instant::now();
    let mut engine = LocalEngine::load(&root, DEFAULT_MODEL, live_translator_lib::translate::local_engine::DEFAULT_TOKENIZER)
        .unwrap_or_else(|e| panic!("引擎加载失败: {e}"));
    eprintln!("模型加载完成: {:.1}s", t0.elapsed().as_secs_f32());

    for round in 1..=3 {
        let t0 = Instant::now();
        match engine.translate(text, target) {
            Ok(out) => eprintln!(
                "第{round}次 [{:.2}s] {}",
                t0.elapsed().as_secs_f32(),
                out
            ),
            Err(e) => eprintln!("第{round}次失败: {e}"),
        }
    }
}

fn cmd_probe() {
    let devices = live_translator_lib::audio::enumerate_devices().expect("枚举失败");
    let ids: Vec<String> = devices.iter().map(|d| d.id.clone()).collect();
    println!(
        "并行信号测试中（{} 设备 × 700ms，请让有声音的源保持出声）…",
        ids.len()
    );
    let t0 = Instant::now();
    let peaks: std::collections::HashMap<String, f32> =
        live_translator_lib::audio::probe::probe_parallel(&ids).into_iter().collect();
    let th = live_translator_lib::audio::probe::SIGNAL_THRESHOLD;
    let mut rows: Vec<_> = devices
        .iter()
        .map(|d| (d, peaks.get(&d.id).copied().unwrap_or(0.0)))
        .collect();
    rows.sort_by(|a, b| {
        let a_on = a.1 >= th;
        let b_on = b.1 >= th;
        b_on.cmp(&a_on).then(b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal))
    });
    for (d, p) in rows {
        let mark = if p >= th { "🔊" } else { "  " };
        println!(
            "{mark} {:>4.0}%  [{:8}] {}  ({})",
            p * 100.0,
            format!("{:?}", d.kind),
            d.name,
            d.id
        );
    }
    println!("--- 完成，耗时 {:.2}s ---", t0.elapsed().as_secs_f32());
}

fn model_root(dir: Option<&str>) -> PathBuf {
    dir.map(PathBuf::from)
        .or_else(|| std::env::var("LIVE_TRANSLATOR_MODELS_DIR").ok().map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from("models"))
}

fn ms_str(ms: u64) -> String {
    format!("{:.*}s", 1, ms as f64 / 1000.0)
}

fn print_segment(engine: &SenseVoiceEngine, start_ms: u64, end_ms: u64, samples: &[f32]) {
    match engine.transcribe(samples) {
        Ok(cand) if !cand.text.is_empty() => {
            let lang = cand.lang.as_deref().unwrap_or("--");
            println!("[{lang}] {} ~ {}  {}", ms_str(start_ms), ms_str(end_ms), cand.text);
        }
        Ok(_) => {}
        Err(e) => eprintln!("识别失败: {e}"),
    }
}

fn cmd_list() {
    match audio::enumerate_devices() {
        Ok(devices) => {
            println!("发现 {} 个可采集音频源:", devices.len());
            for d in devices {
                let kind = match d.kind {
                    live_translator_lib::audio::DeviceKind::Microphone => "麦克风",
                    live_translator_lib::audio::DeviceKind::Loopback => "系统声音",
                    live_translator_lib::audio::DeviceKind::Monitor => "系统声音",
                    live_translator_lib::audio::DeviceKind::Application => "应用音频",
                    _ => "其他",
                };
                let mark = if d.is_default { "*" } else { " " };
                println!(
                    "{mark} [{kind}] {}  ({}Hz, {}ch, id={})",
                    d.name, d.sample_rate, d.channels, d.id
                );
            }
        }
        Err(e) => {
            eprintln!("枚举失败: {e}");
            std::process::exit(1);
        }
    }
}

fn cmd_decode(wav_path: &str, model_dir: Option<&str>) {
    let root = model_root(model_dir);
    println!("模型目录: {}", root.display());

    let wave = sherpa_onnx::Wave::read(wav_path).unwrap_or_else(|| {
        eprintln!("读取 WAV 失败: {wav_path}");
        std::process::exit(1);
    });
    println!(
        "WAV: {}Hz, {} samples",
        wave.sample_rate(),
        wave.num_samples()
    );

    let samples = if wave.sample_rate() as i32 == live_translator_lib::asr::vad::SAMPLE_RATE {
        wave.samples().to_vec()
    } else {
        let mut r =
            audio::resample::Resampler::new(wave.sample_rate() as u32, 16_000);
        r.process(wave.samples())
    };

    let t0 = Instant::now();
    let mut segmenter =
        Segmenter::new(&VadParams::default(), &root.join("silero_vad.onnx"))
            .unwrap_or_else(|e| panic!("VAD 初始化失败: {e}"));
    let engine = SenseVoiceEngine::load(&root, "auto")
        .unwrap_or_else(|e| panic!("模型加载失败: {e}"));

    let mut count = 0;
    for seg in segmenter.feed(&samples) {
        print_segment(&engine, seg.start_ms, seg.end_ms, &seg.samples);
        count += 1;
    }
    for seg in segmenter.flush() {
        print_segment(&engine, seg.start_ms, seg.end_ms, &seg.samples);
        count += 1;
    }
    println!(
        "--- 共 {count} 句，总耗时 {:.2}s（含模型加载） ---",
        t0.elapsed().as_secs_f32()
    );
}

fn cmd_capture(model_dir: Option<&str>, device_sub: Option<&str>, seconds: u64) {
    let root = model_root(model_dir);
    let source = match device_sub.filter(|s| !s.is_empty()) {
        Some(sub) => {
            let devices = audio::enumerate_devices().expect("枚举失败");
            // 同名设备（声卡输入/输出描述名相同）优先匹配麦克风
            let found = devices
                .iter()
                .filter(|d| d.name.contains(sub))
                .max_by_key(|d| matches!(d.kind, audio::DeviceKind::Microphone))
                .unwrap_or_else(|| panic!("未找到包含 \"{sub}\" 的设备"));
            println!("设备: {} ({})", found.name, found.id);
            audio::resolve_source(&found.id).expect("打开设备失败")
        }
        None => audio::default_source().expect("打开默认设备失败"),
    };
    println!("采集设备: {}（{} 秒后自动结束，请说话）", source.desc.name, seconds);

    let engine = SenseVoiceEngine::load(&root, "auto")
        .unwrap_or_else(|e| panic!("模型加载失败: {e}"));

    let (tx, rx) = mpsc::channel::<Vec<f32>>();
    let stream = match &source.device {
        audio::SourceDevice::Cpal(dev) => audio::capture::build_input_stream(
            dev,
            tx,
            std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        )
        .unwrap_or_else(|e| panic!("打开采集流失败: {e}")),
        _ => panic!("该源类型在 smoke 下暂不支持（Windows 环回请用应用内测试）"),
    };
    stream.stream.play().expect("启动采集流失败");
    println!("原生采样率: {}Hz", stream.native_rate);

    let mut resampler = audio::resample::Resampler::new(stream.native_rate, 16_000);
    let mut segmenter = Segmenter::new(&VadParams::default(), &root.join("silero_vad.onnx"))
        .unwrap_or_else(|e| panic!("VAD 初始化失败: {e}"));

    let deadline = Instant::now() + Duration::from_secs(seconds);
    while Instant::now() < deadline {
        match rx.recv_timeout(Duration::from_millis(200)) {
            Ok(chunk) => {
                let mono = resampler.process(&chunk);
                for seg in segmenter.feed(&mono) {
                    print_segment(&engine, seg.start_ms, seg.end_ms, &seg.samples);
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
    for seg in segmenter.flush() {
        print_segment(&engine, seg.start_ms, seg.end_ms, &seg.samples);
    }
    drop(stream);
    println!("--- 采集结束 ---");
}
