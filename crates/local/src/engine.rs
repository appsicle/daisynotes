//! The llama.cpp side: backend init, resident model, per-request context,
//! grammar-constrained decode loop.
//!
//! The backend and model load lazily on the first request and stay resident;
//! a fresh `LlamaContext` (n_ctx 8192) is created per request — cheap next to
//! generation, and it guarantees a clean KV cache every time. Everything is
//! fallible into [`LocalError::Engine`]; nothing panics.

use std::num::NonZeroU32;
use std::time::{Duration, Instant};

use llama_cpp_2::context::params::LlamaContextParams;
use llama_cpp_2::llama_backend::LlamaBackend;
use llama_cpp_2::llama_batch::LlamaBatch;
use llama_cpp_2::model::params::LlamaModelParams;
use llama_cpp_2::model::{AddBos, LlamaModel};
use llama_cpp_2::sampling::LlamaSampler;

use crate::{LocalError, LocalModel, grammar, model_path};

/// Context window for generation.
const N_CTX: u32 = 8192;

/// Hard cap on newly generated tokens. The grammar bounds every string
/// field, so a full two-note reply tops out around ~500 tokens; 768 leaves
/// comfortable headroom while still bounding a pathological decode.
const MAX_NEW_TOKENS: usize = 768;

/// Sampling temperature under the grammar. Kept low: the muse is
/// informational, and terse factual output wants little randomness (the
/// repeat penalty in the sampler chain guards against small-model looping).
const TEMPERATURE: f32 = 0.5;

/// Nucleus sampling cutoff.
const TOP_P: f32 = 0.95;

/// Offload everything to Metal.
const N_GPU_LAYERS: u32 = 1000;

/// A short pause after each token's GPU decode so the app's Metal renderer
/// can present a frame between forward passes. Without it a long generation
/// keeps the GPU saturated and the UI (e.g. switching entries) stalls until
/// the reply lands. ~2ms/token is invisible against a multi-second decode but
/// hands the display a reliable window each step.
const UI_BREATHE: Duration = Duration::from_millis(2);

/// A resident llama.cpp model (plus the process-wide backend token).
pub struct Engine {
    backend: LlamaBackend,
    model: LlamaModel,
    /// Which [`LocalModel`] is loaded, so the worker can reload when a
    /// better one lands on disk.
    pub which: LocalModel,
}

impl Engine {
    /// Load `which` from disk, fully offloaded to Metal.
    pub fn load(which: LocalModel) -> Result<Engine, LocalError> {
        let backend = init_backend()?;
        let path = model_path(which);
        let params = LlamaModelParams::default().with_n_gpu_layers(N_GPU_LAYERS);
        let started = Instant::now();
        let model = LlamaModel::load_from_file(&backend, &path, &params)
            .map_err(|err| engine_err("model load failed", &err))?;
        tracing::info!(
            model = which.display_name(),
            elapsed_ms = started.elapsed().as_millis() as u64,
            "daisynotes-local: model loaded"
        );
        Ok(Engine {
            backend,
            model,
            which,
        })
    }

    /// Run one grammar-constrained generation over `prompt_text` and return
    /// the raw generated string (without the prompt).
    pub fn generate(&self, prompt_text: &str) -> Result<String, LocalError> {
        let tokens = self
            .model
            .str_to_token(prompt_text, AddBos::Always)
            .map_err(|err| engine_err("tokenization failed", &err))?;
        if tokens.is_empty() {
            return Err(LocalError::Engine("empty prompt".to_string()));
        }
        if tokens.len() + MAX_NEW_TOKENS >= N_CTX as usize {
            return Err(LocalError::Engine(format!(
                "prompt too long: {} tokens",
                tokens.len()
            )));
        }

        let ctx_params = LlamaContextParams::default()
            .with_n_ctx(NonZeroU32::new(N_CTX))
            .with_n_batch(N_CTX);
        let mut ctx = self
            .model
            .new_context(&self.backend, ctx_params)
            .map_err(|err| engine_err("context creation failed", &err))?;

        let mut sampler = self.build_sampler()?;

        let mut batch = LlamaBatch::new(tokens.len().max(1), 1);
        let last = tokens.len() - 1;
        for (i, token) in tokens.iter().enumerate() {
            batch
                .add(*token, i as i32, &[0], i == last)
                .map_err(|err| engine_err("batch add failed", &err))?;
        }
        ctx.decode(&mut batch)
            .map_err(|err| engine_err("prompt decode failed", &err))?;

        let started = Instant::now();
        let mut out_bytes: Vec<u8> = Vec::with_capacity(1024);
        for n_cur in (tokens.len() as i32..).take(MAX_NEW_TOKENS) {
            let token = sampler.sample(&ctx, batch.n_tokens() - 1);
            if self.model.is_eog_token(token) {
                break;
            }
            match self.model.token_to_piece_bytes(token, 64, false, None) {
                Ok(bytes) => out_bytes.extend_from_slice(&bytes),
                Err(err) => {
                    tracing::warn!(%err, "daisynotes-local: token decode failed; skipping piece");
                }
            }
            batch.clear();
            batch
                .add(token, n_cur, &[0], true)
                .map_err(|err| engine_err("batch add failed", &err))?;
            ctx.decode(&mut batch)
                .map_err(|err| engine_err("token decode step failed", &err))?;
            // Let the display present between forward passes (see UI_BREATHE).
            std::thread::sleep(UI_BREATHE);
        }

        let mut text = String::from_utf8_lossy(&out_bytes).into_owned();
        // Belt and braces: the grammar should stop us, but trim a literal
        // end-of-turn marker if one slipped through as plaintext.
        if let Some(idx) = text.find("<end_of_turn>") {
            text.truncate(idx);
        }
        tracing::debug!(
            prompt_tokens = tokens.len(),
            generated_chars = text.len(),
            elapsed_ms = started.elapsed().as_millis() as u64,
            "daisynotes-local: generation complete"
        );
        Ok(text)
    }

    /// Grammar → temperature → top-p → dist, exactly one token out.
    fn build_sampler(&self) -> Result<LlamaSampler, LocalError> {
        let grammar = LlamaSampler::grammar(&self.model, grammar::GRAMMAR, grammar::GRAMMAR_ROOT)
            .map_err(|err| engine_err("grammar compile failed", &err))?;
        let seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.subsec_nanos())
            .unwrap_or(0x5EED);
        Ok(LlamaSampler::chain(
            [
                grammar,
                // Small models loop ("hjhjhj…") without a repeat penalty.
                LlamaSampler::penalties(128, 1.18, 0.02, 0.0),
                LlamaSampler::temp(TEMPERATURE),
                LlamaSampler::top_p(TOP_P, 1),
                LlamaSampler::dist(seed),
            ],
            true,
        ))
    }
}

/// Initialize the process-wide llama backend, tolerating double-init (the
/// backend type is a zero-sized proof token).
fn init_backend() -> Result<LlamaBackend, LocalError> {
    match LlamaBackend::init() {
        Ok(backend) => Ok(backend),
        Err(llama_cpp_2::LlamaCppError::BackendAlreadyInitialized) => Ok(LlamaBackend {}),
        Err(err) => Err(engine_err("backend init failed", &err)),
    }
}

fn engine_err(what: &str, err: &dyn std::fmt::Display) -> LocalError {
    tracing::warn!(%err, "daisynotes-local: {what}");
    LocalError::Engine(format!("{what}: {err}"))
}
