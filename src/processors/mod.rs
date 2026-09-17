//! Streaming line processors and the chain that runs them.

pub mod ansi;
pub mod dedupe;
pub mod diff;
pub mod stacktrace;

use crate::detector::FormatType;

/// Destination for lines a processor emits.
pub trait LineSink {
    fn emit(&mut self, line: &str);
}

/// A streaming transformation over lines.
///
/// A processor may emit zero, one, or many lines per input line; state it holds
/// across lines (an open dedupe run, a half-parsed trace) must be released in
/// [`LineProcessor::finish`].
pub trait LineProcessor {
    fn on_line(&mut self, line: &str, out: &mut dyn LineSink);

    /// Flush pending state at end of input.
    fn finish(&mut self, out: &mut dyn LineSink) {
        let _ = out;
    }

    fn name(&self) -> &'static str;

    /// Lines worth surfacing separately (e.g. the exception a trace ended on).
    fn summary(&self) -> Vec<String> {
        Vec::new()
    }
}

/// Whole-string API mirroring the PRD's interface.
pub trait LogProcessor {
    fn process(&mut self, input: &str) -> ProcessedOutput;
}

/// Result of a compression run.
#[derive(Debug, Clone, Default)]
pub struct ProcessedOutput {
    pub content: String,
    pub original_line_count: usize,
    pub processed_line_count: usize,
    pub detected_type: FormatType,
    /// Error lines the stack-trace processor considered the root cause.
    pub error_summary: Vec<String>,
    pub stats: crate::utils::stats::ReductionStats,
}

/// A growable line buffer that reuses its `String` allocations across resets.
#[derive(Debug, Default)]
pub struct VecSink {
    lines: Vec<String>,
    len: usize,
}

impl VecSink {
    pub fn reset(&mut self) {
        self.len = 0;
    }

    pub fn as_slice(&self) -> &[String] {
        &self.lines[..self.len]
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
}

impl LineSink for VecSink {
    fn emit(&mut self, line: &str) {
        if self.len < self.lines.len() {
            let slot = &mut self.lines[self.len];
            slot.clear();
            slot.push_str(line);
        } else {
            self.lines.push(line.to_string());
        }
        self.len += 1;
    }
}

impl LineSink for Vec<String> {
    fn emit(&mut self, line: &str) {
        self.push(line.to_string());
    }
}

/// Runs processors back to back, feeding each one's output into the next.
pub struct Chain {
    procs: Vec<Box<dyn LineProcessor>>,
    scratch: Vec<VecSink>,
}

impl Chain {
    pub fn new(procs: Vec<Box<dyn LineProcessor>>) -> Self {
        let scratch = (0..procs.len()).map(|_| VecSink::default()).collect();
        Self { procs, scratch }
    }

    pub fn is_empty(&self) -> bool {
        self.procs.is_empty()
    }

    pub fn on_line(&mut self, line: &str, out: &mut dyn LineSink) {
        self.feed(0, std::slice::from_ref(&line), out);
    }

    /// Flush every processor, letting each one's tail flow through the rest.
    pub fn finish(&mut self, out: &mut dyn LineSink) {
        for stage in 0..self.procs.len() {
            let mut sink = std::mem::take(&mut self.scratch[stage]);
            sink.reset();
            self.procs[stage].finish(&mut sink);
            if !sink.is_empty() {
                self.feed(stage + 1, sink.as_slice(), out);
            }
            self.scratch[stage] = sink;
        }
    }

    pub fn summary(&self) -> Vec<String> {
        self.procs.iter().flat_map(|p| p.summary()).collect()
    }

    fn feed<S: AsRef<str>>(&mut self, stage: usize, lines: &[S], out: &mut dyn LineSink) {
        if stage >= self.procs.len() {
            for line in lines {
                out.emit(line.as_ref());
            }
            return;
        }
        // Taking the buffer out of `self` lets us borrow it while the chain is
        // mutably borrowed for the recursive call; it is restored afterwards so
        // the allocation is reused on the next line.
        let mut sink = std::mem::take(&mut self.scratch[stage]);
        sink.reset();
        for line in lines {
            self.procs[stage].on_line(line.as_ref(), &mut sink);
        }
        if !sink.is_empty() {
            self.feed(stage + 1, sink.as_slice(), out);
        }
        self.scratch[stage] = sink;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Emits each line twice, plus one line at finish.
    struct Doubler;
    impl LineProcessor for Doubler {
        fn on_line(&mut self, line: &str, out: &mut dyn LineSink) {
            out.emit(line);
            out.emit(line);
        }
        fn finish(&mut self, out: &mut dyn LineSink) {
            out.emit("tail");
        }
        fn name(&self) -> &'static str {
            "doubler"
        }
    }

    struct Upper;
    impl LineProcessor for Upper {
        fn on_line(&mut self, line: &str, out: &mut dyn LineSink) {
            out.emit(&line.to_uppercase());
        }
        fn name(&self) -> &'static str {
            "upper"
        }
    }

    #[test]
    fn chain_feeds_each_stage_into_the_next() {
        let mut chain = Chain::new(vec![Box::new(Doubler), Box::new(Upper)]);
        let mut out: Vec<String> = Vec::new();
        chain.on_line("a", &mut out);
        assert_eq!(out, vec!["A", "A"]);
    }

    #[test]
    fn finish_output_flows_through_later_stages() {
        let mut chain = Chain::new(vec![Box::new(Doubler), Box::new(Upper)]);
        let mut out: Vec<String> = Vec::new();
        chain.on_line("a", &mut out);
        chain.finish(&mut out);
        assert_eq!(out, vec!["A", "A", "TAIL"]);
    }

    #[test]
    fn vec_sink_reuses_allocations_after_reset() {
        let mut sink = VecSink::default();
        sink.emit("first");
        sink.reset();
        sink.emit("second");
        assert_eq!(sink.as_slice(), ["second"]);
        assert_eq!(
            sink.lines.len(),
            1,
            "allocation should be reused, not grown"
        );
    }
}
