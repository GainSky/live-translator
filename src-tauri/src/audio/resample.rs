//! 采样格式归一化：任意声道/采样率 → 16kHz 单声道 f32（sherpa-onnx 输入要求）
//!
//! 线性插值重采样：零依赖、CPU 开销极小，对 16k 目标的语音识别质量足够。
//! 若 M1 实测质量不足，替换为 rubato（sinc）实现——接口保持不变。

/// 有状态线性重采样器（按块喂入，跨块连续）
pub struct Resampler {
    from_rate: u32,
    to_rate: u32,
    /// 下一个输出样本在输入时间轴上的绝对位置（单位：输入采样）
    next_out_pos: f64,
    /// 本块之前累计消费的输入样本数
    consumed_before: u64,
    /// 上一块最后一个样本（跨块插值边界用）
    last_sample: Option<f32>,
}

impl Resampler {
    pub fn new(from_rate: u32, to_rate: u32) -> Self {
        Self {
            from_rate,
            to_rate,
            next_out_pos: 0.0,
            consumed_before: 0,
            last_sample: None,
        }
    }

    pub fn process(&mut self, input: &[f32]) -> Vec<f32> {
        let n = input.len() as u64;
        if n == 0 {
            return Vec::new();
        }
        if self.from_rate == self.to_rate {
            self.consumed_before += n;
            self.last_sample = input.last().copied();
            return input.to_vec();
        }

        let step = self.from_rate as f64 / self.to_rate as f64;
        let mut out = Vec::with_capacity((n as f64 / step) as usize + 2);
        let mut t = self.next_out_pos;

        loop {
            let idx = t.floor();
            let left = idx as u64;
            if left + 1 >= self.consumed_before + n {
                break;
            }
            let frac = (t - idx) as f32;
            let s0 = if left >= self.consumed_before {
                input[(left - self.consumed_before) as usize]
            } else {
                self.last_sample.unwrap_or(0.0)
            };
            let s1 = input[(left + 1 - self.consumed_before) as usize];
            out.push(s0 + (s1 - s0) * frac);
            t += step;
        }

        self.next_out_pos = t;
        self.consumed_before += n;
        self.last_sample = input.last().copied();
        out
    }
}

/// 多声道 → 单声道（均值下混）
pub fn to_mono(input: &[f32], channels: u16) -> Vec<f32> {
    let ch = channels.max(1) as usize;
    if ch == 1 {
        return input.to_vec();
    }
    input
        .chunks(ch)
        .map(|frame| frame.iter().sum::<f32>() / frame.len() as f32)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn passthrough_same_rate() {
        let mut r = Resampler::new(16000, 16000);
        let input: Vec<f32> = (0..1600).map(|i| i as f32).collect();
        assert_eq!(r.process(&input).len(), 1600);
    }

    #[test]
    fn downsample_48k_to_16k_triples_ratio() {
        let mut r = Resampler::new(48000, 16000);
        let input = vec![0.5f32; 4800]; // 0.1s @48k → 应输出 ≈0.1s @16k
        let out = r.process(&input);
        assert!((out.len() as i64 - 1600).abs() <= 2, "len={}", out.len());
        // 恒定信号插值后仍恒定
        assert!(out.iter().all(|&v| (v - 0.5).abs() < 1e-6));
    }

    #[test]
    fn mono_downmix() {
        let stereo = vec![0.0f32, 1.0, 0.0, 1.0];
        assert_eq!(to_mono(&stereo, 2), vec![0.5, 0.5]);
    }
}
