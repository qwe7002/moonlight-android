//! Build script for moonlight-core-rs
//!
//! This script compiles moonlight-common-c and libopus using the cc crate for reliable cross-compilation.
//! It automatically downloads the latest moonlight-common-c source from GitHub if not present.

use std::env;
use std::path::PathBuf;
use std::process::Command;

/// Check if this is a release build
fn is_release_build() -> bool {
    env::var("PROFILE").map(|p| p == "release").unwrap_or(false)
}

/// Get optimization flags based on build profile
fn get_optimization_flags() -> Vec<&'static str> {
    if is_release_build() {
        vec!["-O3", "-ffast-math", "-fno-finite-math-only"]
    } else {
        vec!["-O0", "-g"]
    }
}

/// Apply common build settings to a cc::Build instance
fn apply_common_settings(build: &mut cc::Build) {
    let opt_flags = get_optimization_flags();
    for flag in &opt_flags {
        build.flag(flag);
    }

    // ARM64 NEON is always available on aarch64
    build.flag("-march=armv8-a");

    // Enable parallel compilation
    build.flag("-pipe");

    if is_release_build() {
        // Enable function/data sections for better dead code elimination
        build.flag("-ffunction-sections");
        build.flag("-fdata-sections");
        // Enable link-time optimization hints
        build.flag("-flto=thin");
        // Vectorization for better SIMD utilization
        build.flag("-ftree-vectorize");
    }
}

fn main() {
    let target = env::var("TARGET").unwrap();
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());

    // Use a stable path for downloaded dependencies (not OUT_DIR which changes with each build)
    let deps_dir = manifest_dir.join("target").join("deps");
    std::fs::create_dir_all(&deps_dir).ok();

    let moonlight_common_c_dir = deps_dir.join("moonlight-common-c");

    // Tell cargo to re-run if our source files change
    println!("cargo:rerun-if-changed=src/ffi.rs");

    // Only build for Android targets
    if !target.contains("android") {
        return;
    }

    // Only support arm64-v8a (aarch64) - skip other architectures entirely
    if !target.contains("aarch64") {
        eprintln!("Warning: Only arm64-v8a is supported, skipping build for {}", target);
        return;
    }

    // Download moonlight-common-c if not present
    download_moonlight_common_c(&moonlight_common_c_dir);


    // Build opus library
    build_opus();

    // Build enet library
    let enet_dir = moonlight_common_c_dir.join("enet");
    let mut enet_build = cc::Build::new();
    enet_build
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
        .warnings(false);
    apply_common_settings(&mut enet_build);
    enet_build.compile("enet");

    // Build reed-solomon library
    let rs_dir = moonlight_common_c_dir.join("reedsolomon");
    let mut rs_build = cc::Build::new();
    rs_build
        .file(rs_dir.join("rs.c"))
        .include(&rs_dir)
        .warnings(false);
    apply_common_settings(&mut rs_build);
    rs_build.compile("reedsolomon");

    // Build moonlight-common-c library (without PlatformCrypto.c - using Rust ring instead)
    let src_dir = moonlight_common_c_dir.join("src");
    let mut mlc_build = cc::Build::new();
    mlc_build
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
        .warnings(false);
    apply_common_settings(&mut mlc_build);
    mlc_build.compile("moonlight-common-c");

    // Link Android system libraries
    println!("cargo:rustc-link-lib=log");
}

/// Build libopus using cc crate
fn build_opus() {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());

    // Use stable deps directory for downloaded source
    let deps_dir = manifest_dir.join("target").join("deps");
    std::fs::create_dir_all(&deps_dir).ok();

    // First, try to find opus source in the project's opus directory
    let project_opus_dir = manifest_dir.join("opus");
    let opus_dir = if project_opus_dir.exists() {
        project_opus_dir
    } else {
        // Download opus source directly to deps directory
        download_opus(&deps_dir)
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
    let silk_fixed_dir = silk_dir.join("fixed");
    let src_dir = opus_dir.join("src");

    let mut build = cc::Build::new();

    // Include directories
    build
        .include(&opus_dir)
        .include(&include_dir)
        .include(&celt_dir)
        .include(&silk_dir)
        .include(&silk_float_dir)
        .include(&silk_fixed_dir)
        .include(&src_dir);

    // Defines for build
    build
        .define("OPUS_BUILD", None)
        .define("HAVE_LRINTF", "1")
        .define("VAR_ARRAYS", None)
        .define("FIXED_POINT", None);

    // ARM64 NEON intrinsic directory
    let celt_arm_dir = celt_dir.join("arm");
    let silk_arm_dir = silk_dir.join("arm");

    // Check if NEON sources are available and add them
    let has_neon = celt_arm_dir.exists() && silk_arm_dir.exists();
    if has_neon {
        build
            .include(&celt_arm_dir)
            .include(&silk_arm_dir)
            // Enable ARM64 intrinsics for better performance
            .define("OPUS_ARM_ASM", None)
            .define("OPUS_ARM_MAY_HAVE_NEON_INTR", None)
            .define("OPUS_ARM_PRESUME_NEON_INTR", None);

        // CELT ARM NEON sources
        let celt_arm_sources = [
            "arm_celt_map.c",
            "celt_neon_intr.c",
            "pitch_neon_intr.c",
        ];
        for src in &celt_arm_sources {
            let path = celt_arm_dir.join(src);
            if path.exists() {
                build.file(path);
            }
        }

        // SILK ARM NEON sources
        let silk_arm_sources = [
            "arm_silk_map.c",
            "biquad_alt_neon_intr.c",
            "LPC_inv_pred_gain_neon_intr.c",
            "NSQ_del_dec_neon_intr.c",
            "NSQ_neon.c",
        ];
        for src in &silk_arm_sources {
            let path = silk_arm_dir.join(src);
            if path.exists() {
                build.file(path);
            }
        }
    }


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

    // SILK fixed sources (for FIXED_POINT mode with ARM NEON optimization)
    let silk_fixed_sources = [
        "apply_sine_window_FIX.c",
        "autocorr_FIX.c",
        "burg_modified_FIX.c",
        "corrMatrix_FIX.c",
        "encode_frame_FIX.c",
        "find_LPC_FIX.c",
        "find_LTP_FIX.c",
        "find_pitch_lags_FIX.c",
        "find_pred_coefs_FIX.c",
        "k2a_FIX.c",
        "k2a_Q16_FIX.c",
        "LTP_analysis_filter_FIX.c",
        "LTP_scale_ctrl_FIX.c",
        "noise_shape_analysis_FIX.c",
        "pitch_analysis_core_FIX.c",
        "process_gains_FIX.c",
        "regularize_correlations_FIX.c",
        "residual_energy_FIX.c",
        "residual_energy16_FIX.c",
        "schur_FIX.c",
        "schur64_FIX.c",
        "vector_ops_FIX.c",
        "warped_autocorrelation_FIX.c",
    ];
    for src in &silk_fixed_sources {
        let path = silk_fixed_dir.join(src);
        if path.exists() {
            build.file(path);
        }
    }

    // Main opus sources
    let opus_sources = [
        "analysis.c",
        "extensions.c",
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
    apply_common_settings(&mut build);
    build.compile("opus");

    // Tell cargo where to find opus headers for the Rust bindings
    println!("cargo:include={}", include_dir.display());
    println!("cargo:root={}", out_dir.display());
}


/// Download opus source code
fn download_opus(out_dir: &PathBuf) -> PathBuf {
    use std::process::Command;

    let opus_dir = out_dir.join("opus");

    if opus_dir.exists() && opus_dir.join("include").exists() {
        return opus_dir;
    }

    // Clone opus from git
    let opus_version = "v1.6.1";
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

/// Download moonlight-common-c from GitHub
fn download_moonlight_common_c(target_dir: &PathBuf) {
    let repo_url = "https://github.com/moonlight-stream/moonlight-common-c.git";

    // If directory already exists with source files, skip download
    if target_dir.exists() && target_dir.join("src").exists() {
        return;
    }

    println!("cargo:warning=Downloading moonlight-common-c from GitHub...");

    // Remove directory if it exists but is incomplete
    if target_dir.exists() {
        let _ = std::fs::remove_dir_all(target_dir);
    }

    let status = Command::new("git")
        .args([
            "clone",
            "--recursive",
            "--depth", "1",
            repo_url,
        ])
        .arg(target_dir)
        .status();

    match status {
        Ok(s) if s.success() => {
            println!("cargo:warning=Successfully downloaded moonlight-common-c to {:?}", target_dir);
        }
        Ok(s) => {
            panic!("Failed to clone moonlight-common-c: git exited with status {}", s);
        }
        Err(e) => {
            panic!("Failed to run git to clone moonlight-common-c: {}", e);
        }
    }
}
