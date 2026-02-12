//! Build script for moonlight-core-rs
//!
//! This script compiles moonlight-common-c and libopus using the cc crate for reliable cross-compilation.

use std::env;
use std::path::PathBuf;

fn main() {
    let target = env::var("TARGET").unwrap();
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let moonlight_common_c_dir = manifest_dir.join("moonlight-common-c");

    // Tell cargo to re-run if our source files change
    println!("cargo:rerun-if-changed=src/ffi.rs");
    println!("cargo:rerun-if-changed={}", moonlight_common_c_dir.display());

    // Only build for Android targets
    if !target.contains("android") {
        return;
    }

    // Only support arm64-v8a (aarch64) - skip other architectures entirely
    if !target.contains("aarch64") {
        eprintln!("Warning: Only arm64-v8a is supported, skipping build for {}", target);
        return;
    }

    // Build opus library
    build_opus();

    // Build enet library
    let enet_dir = moonlight_common_c_dir.join("enet");
    cc::Build::new()
        .file(enet_dir.join("callbacks.c"))
        .file(enet_dir.join("compress.c"))
        .file(enet_dir.join("host.c"))
        .file(enet_dir.join("list.c"))
        .file(enet_dir.join("packet.c"))
        .file(enet_dir.join("peer.c"))
        .file(enet_dir.join("protocol.c"))
        .file(enet_dir.join("unix.c"))
        .include(enet_dir.join("include"))
        .define("HAS_SOCKLEN_T", "1")
        .define("HAVE_CLOCK_GETTIME", "1")
        .warnings(false)
        .compile("enet");

    // Build reed-solomon library
    let rs_dir = moonlight_common_c_dir.join("reedsolomon");
    cc::Build::new()
        .file(rs_dir.join("rs.c"))
        .include(&rs_dir)
        .warnings(false)
        .compile("reedsolomon");

    // Build moonlight-common-c library (without PlatformCrypto.c - using Rust ring instead)
    let src_dir = moonlight_common_c_dir.join("src");
    cc::Build::new()
        .file(src_dir.join("AudioStream.c"))
        .file(src_dir.join("ByteBuffer.c"))
        .file(src_dir.join("Connection.c"))
        .file(src_dir.join("ConnectionTester.c"))
        .file(src_dir.join("ControlStream.c"))
        .file(src_dir.join("FakeCallbacks.c"))
        .file(src_dir.join("InputStream.c"))
        .file(src_dir.join("LinkedBlockingQueue.c"))
        .file(src_dir.join("Misc.c"))
        .file(src_dir.join("Platform.c"))
        // PlatformCrypto.c is excluded - crypto is handled by Rust ring crate
        .file(src_dir.join("PlatformSockets.c"))
        .file(src_dir.join("RtpAudioQueue.c"))
        .file(src_dir.join("RtpVideoQueue.c"))
        .file(src_dir.join("RtspConnection.c"))
        .file(src_dir.join("RtspParser.c"))
        .file(src_dir.join("SdpGenerator.c"))
        .file(src_dir.join("SimpleStun.c"))
        .file(src_dir.join("VideoDepacketizer.c"))
        .file(src_dir.join("VideoStream.c"))
        .include(&src_dir)
        .include(enet_dir.join("include"))
        .include(&rs_dir)
        .define("HAS_SOCKLEN_T", "1")
        .define("LC_ANDROID", None)
        .define("HAVE_CLOCK_GETTIME", "1")
        .warnings(false)
        .compile("moonlight-common-c");

    // Link Android system libraries
    println!("cargo:rustc-link-lib=log");
}

/// Build libopus using cc crate
fn build_opus() {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());

    // First, try to find opus source in the project's opus directory
    let project_opus_dir = manifest_dir.join("opus");
    let opus_dir = if project_opus_dir.exists() {
        project_opus_dir
    } else {
        // Try to find in cargo registry (from audiopus_sys or opus-sys)
        let home = env::var("CARGO_HOME")
            .or_else(|_| env::var("HOME").map(|h| format!("{}/.cargo", h)))
            .unwrap_or_else(|_| {
                // Windows fallback
                let userprofile = env::var("USERPROFILE").unwrap_or_default();
                format!("{}/.cargo", userprofile)
            });

        let registry_src = PathBuf::from(&home).join("registry/src");
        match find_opus_source(&registry_src) {
            Some(dir) => dir,
            None => {
                // Download opus source
                download_opus(&out_dir)
            }
        }
    };

    if !opus_dir.exists() {
        eprintln!("Warning: Could not find opus source at {:?}, opus decoding may not work", opus_dir);
        return;
    }

    println!("cargo:rerun-if-changed={}", opus_dir.display());

    let include_dir = opus_dir.join("include");
    let celt_dir = opus_dir.join("celt");
    let silk_dir = opus_dir.join("silk");
    let silk_float_dir = silk_dir.join("float");
    let src_dir = opus_dir.join("src");

    let mut build = cc::Build::new();

    // Include directories
    build
        .include(&opus_dir)
        .include(&include_dir)
        .include(&celt_dir)
        .include(&silk_dir)
        .include(&silk_float_dir)
        .include(&src_dir);

    // Defines for floating-point build
    build
        .define("OPUS_BUILD", None)
        .define("HAVE_LRINTF", "1")
        .define("FLOAT_APPROX", None)
        .define("VAR_ARRAYS", None);

    // CELT sources
    let celt_sources = [
        "bands.c",
        "celt.c",
        "celt_decoder.c",
        "celt_encoder.c",
        "celt_lpc.c",
        "cwrs.c",
        "entcode.c",
        "entdec.c",
        "entenc.c",
        "kiss_fft.c",
        "laplace.c",
        "mathops.c",
        "mdct.c",
        "modes.c",
        "pitch.c",
        "quant_bands.c",
        "rate.c",
        "vq.c",
    ];
    for src in &celt_sources {
        let path = celt_dir.join(src);
        if path.exists() {
            build.file(path);
        }
    }

    // SILK sources
    let silk_sources = [
        "A2NLSF.c",
        "ana_filt_bank_1.c",
        "biquad_alt.c",
        "bwexpander.c",
        "bwexpander_32.c",
        "check_control_input.c",
        "CNG.c",
        "code_signs.c",
        "control_audio_bandwidth.c",
        "control_codec.c",
        "control_SNR.c",
        "debug.c",
        "dec_API.c",
        "decode_core.c",
        "decode_frame.c",
        "decode_indices.c",
        "decode_parameters.c",
        "decode_pitch.c",
        "decode_pulses.c",
        "decoder_set_fs.c",
        "enc_API.c",
        "encode_indices.c",
        "encode_pulses.c",
        "gain_quant.c",
        "HP_variable_cutoff.c",
        "init_decoder.c",
        "init_encoder.c",
        "inner_prod_aligned.c",
        "interpolate.c",
        "lin2log.c",
        "log2lin.c",
        "LP_variable_cutoff.c",
        "LPC_analysis_filter.c",
        "LPC_fit.c",
        "LPC_inv_pred_gain.c",
        "NLSF_decode.c",
        "NLSF_del_dec_quant.c",
        "NLSF_encode.c",
        "NLSF_stabilize.c",
        "NLSF_unpack.c",
        "NLSF_VQ.c",
        "NLSF_VQ_weights_laroia.c",
        "NLSF2A.c",
        "NSQ.c",
        "NSQ_del_dec.c",
        "pitch_est_tables.c",
        "PLC.c",
        "process_NLSFs.c",
        "quant_LTP_gains.c",
        "resampler.c",
        "resampler_down2.c",
        "resampler_down2_3.c",
        "resampler_private_AR2.c",
        "resampler_private_down_FIR.c",
        "resampler_private_IIR_FIR.c",
        "resampler_private_up2_HQ.c",
        "resampler_rom.c",
        "shell_coder.c",
        "sigm_Q15.c",
        "sort.c",
        "stereo_decode_pred.c",
        "stereo_encode_pred.c",
        "stereo_find_predictor.c",
        "stereo_LR_to_MS.c",
        "stereo_MS_to_LR.c",
        "stereo_quant_pred.c",
        "sum_sqr_shift.c",
        "table_LSF_cos.c",
        "tables_gain.c",
        "tables_LTP.c",
        "tables_NLSF_CB_NB_MB.c",
        "tables_NLSF_CB_WB.c",
        "tables_other.c",
        "tables_pitch_lag.c",
        "tables_pulses_per_block.c",
        "VAD.c",
        "VQ_WMat_EC.c",
    ];
    for src in &silk_sources {
        let path = silk_dir.join(src);
        if path.exists() {
            build.file(path);
        }
    }

    // SILK float sources
    let silk_float_sources = [
        "apply_sine_window_FLP.c",
        "autocorrelation_FLP.c",
        "burg_modified_FLP.c",
        "bwexpander_FLP.c",
        "corrMatrix_FLP.c",
        "encode_frame_FLP.c",
        "energy_FLP.c",
        "find_LPC_FLP.c",
        "find_LTP_FLP.c",
        "find_pitch_lags_FLP.c",
        "find_pred_coefs_FLP.c",
        "inner_product_FLP.c",
        "k2a_FLP.c",
        "LPC_analysis_filter_FLP.c",
        "LPC_inv_pred_gain_FLP.c",
        "LTP_analysis_filter_FLP.c",
        "LTP_scale_ctrl_FLP.c",
        "noise_shape_analysis_FLP.c",
        "pitch_analysis_core_FLP.c",
        "process_gains_FLP.c",
        "regularize_correlations_FLP.c",
        "residual_energy_FLP.c",
        "scale_copy_vector_FLP.c",
        "scale_vector_FLP.c",
        "schur_FLP.c",
        "sort_FLP.c",
        "warped_autocorrelation_FLP.c",
        "wrappers_FLP.c",
    ];
    for src in &silk_float_sources {
        let path = silk_float_dir.join(src);
        if path.exists() {
            build.file(path);
        }
    }

    // Main opus sources
    let opus_sources = [
        "analysis.c",
        "mapping_matrix.c",
        "mlp.c",
        "mlp_data.c",
        "opus.c",
        "opus_decoder.c",
        "opus_encoder.c",
        "opus_multistream.c",
        "opus_multistream_decoder.c",
        "opus_multistream_encoder.c",
        "opus_projection_decoder.c",
        "opus_projection_encoder.c",
        "repacketizer.c",
    ];
    for src in &opus_sources {
        let path = src_dir.join(src);
        if path.exists() {
            build.file(path);
        }
    }

    build.warnings(false);
    build.compile("opus");

    // Tell cargo where to find opus headers for the Rust bindings
    println!("cargo:include={}", include_dir.display());
    println!("cargo:root={}", out_dir.display());
}

/// Find opus source directory in cargo registry
fn find_opus_source(registry_src: &PathBuf) -> Option<PathBuf> {
    if !registry_src.exists() {
        return None;
    }

    // Look for index.crates.io-* directories
    for entry in std::fs::read_dir(registry_src).ok()? {
        let entry = entry.ok()?;
        let path = entry.path();
        if path.is_dir() {
            // Look for audiopus_sys-* or opus-sys-* directories
            for pkg_entry in std::fs::read_dir(&path).ok()? {
                let pkg_entry = pkg_entry.ok()?;
                let pkg_path = pkg_entry.path();
                if pkg_path.is_dir() {
                    let name = pkg_path.file_name()?.to_str()?;
                    if name.starts_with("audiopus_sys-") || name.starts_with("opus-sys-") {
                        let opus_path = pkg_path.join("opus");
                        if opus_path.exists() && opus_path.join("include").exists() {
                            return Some(opus_path);
                        }
                    }
                }
            }
        }
    }

    None
}

/// Download opus source code
fn download_opus(out_dir: &PathBuf) -> PathBuf {
    use std::process::Command;

    let opus_dir = out_dir.join("opus");

    if opus_dir.exists() && opus_dir.join("include").exists() {
        return opus_dir;
    }

    // Clone opus from git
    let opus_version = "v1.4";
    let status = Command::new("git")
        .args(&["clone", "--depth", "1", "--branch", opus_version,
                "https://github.com/xiph/opus.git"])
        .arg(&opus_dir)
        .status();

    match status {
        Ok(s) if s.success() => {
            println!("cargo:warning=Downloaded opus source to {:?}", opus_dir);
        }
        _ => {
            eprintln!("Failed to download opus source");
        }
    }

    opus_dir
}

