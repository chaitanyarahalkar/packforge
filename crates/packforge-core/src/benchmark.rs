//! Reproducible in-process compression and verification measurements.

use std::time::Instant;

use serde::Serialize;

use crate::{
    ArtifactInfo, BinaryInfo, ContainerError, PackOptions, Profile, classify, pack, verify,
};

/// Maximum measured iterations accepted by one benchmark invocation.
pub const MAX_BENCHMARK_ITERATIONS: u32 = 100;

/// Versioned benchmark report.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BenchmarkReport {
    /// JSON/report schema version.
    pub schema_version: u16,
    /// Number of measured iterations per profile.
    pub iterations: u32,
    /// Unmeasured warm-up iterations per profile.
    pub warmup_iterations: u32,
    /// Original executable length.
    pub original_size: u64,
    /// Original executable BLAKE3 digest.
    pub original_digest: String,
    /// Executable-format classification.
    pub binary: BinaryInfo,
    /// Stable-profile results in fast, balanced, small, auto order.
    pub profiles: Vec<ProfileBenchmark>,
}

/// Measurements and artifact facts for one profile.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProfileBenchmark {
    /// Requested compression policy.
    pub profile: Profile,
    /// Codec selected by the profile.
    pub codec: crate::Codec,
    /// Selected codec level.
    pub codec_level: i32,
    /// Complete container length.
    pub container_size: u64,
    /// Compressed payload length.
    pub payload_size: u64,
    /// Payload/original ratio in basis points.
    pub payload_ratio_basis_points: u32,
    /// Median pack duration in nanoseconds.
    pub pack_nanoseconds_median: u64,
    /// Minimum pack duration in nanoseconds.
    pub pack_nanoseconds_minimum: u64,
    /// Median full-verification duration in nanoseconds.
    pub verify_nanoseconds_median: u64,
    /// Minimum full-verification duration in nanoseconds.
    pub verify_nanoseconds_minimum: u64,
}

/// Benchmarks every stable profile with a warm-up and deterministic-output check.
///
/// Timing uses the current process and monotonic clock. The report intentionally
/// contains raw nanoseconds so callers can retain machine context separately.
///
/// # Errors
///
/// Returns [`ContainerError`] when the iteration count is invalid, the executable
/// cannot be packed and verified, or repeated output is not byte-identical.
pub fn benchmark(
    input: &[u8],
    original_mode: u32,
    iterations: u32,
) -> Result<BenchmarkReport, ContainerError> {
    if iterations == 0 || iterations > MAX_BENCHMARK_ITERATIONS {
        return Err(ContainerError::InvalidIterations {
            actual: iterations,
            maximum: MAX_BENCHMARK_ITERATIONS,
        });
    }
    let binary = classify(input)?;
    let original_digest = blake3::hash(input).to_string();
    let profiles = [
        Profile::Fast,
        Profile::Balanced,
        Profile::Small,
        Profile::Auto,
    ];
    let mut measurements = Vec::with_capacity(profiles.len());
    for profile in profiles {
        measurements.push(benchmark_profile(
            input,
            original_mode,
            iterations,
            profile,
        )?);
    }
    Ok(BenchmarkReport {
        schema_version: 1,
        iterations,
        warmup_iterations: 1,
        original_size: u64::try_from(input.len()).unwrap_or(u64::MAX),
        original_digest,
        binary,
        profiles: measurements,
    })
}

fn benchmark_profile(
    input: &[u8],
    original_mode: u32,
    iterations: u32,
    profile: Profile,
) -> Result<ProfileBenchmark, ContainerError> {
    let options = PackOptions {
        profile,
        allow_larger: true,
    };
    let warmup = pack(input, original_mode, options)?;
    verify(&warmup.bytes)?;

    let mut pack_times = Vec::with_capacity(usize::try_from(iterations).unwrap_or(0));
    let mut verify_times = Vec::with_capacity(usize::try_from(iterations).unwrap_or(0));
    let mut last_info: ArtifactInfo = warmup.info;
    for _ in 0..iterations {
        let started = Instant::now();
        let artifact = pack(input, original_mode, options)?;
        pack_times.push(duration_nanoseconds(started));
        if artifact.bytes != warmup.bytes {
            return Err(ContainerError::NonDeterministic(profile));
        }

        let started = Instant::now();
        last_info = verify(&artifact.bytes)?;
        verify_times.push(duration_nanoseconds(started));
    }

    let pack_minimum = *pack_times.iter().min().unwrap_or(&0);
    let verify_minimum = *verify_times.iter().min().unwrap_or(&0);
    Ok(ProfileBenchmark {
        profile,
        codec: last_info.codec,
        codec_level: last_info.codec_level,
        container_size: last_info.container_size,
        payload_size: last_info.payload_size,
        payload_ratio_basis_points: last_info.payload_ratio_basis_points,
        pack_nanoseconds_median: median(&mut pack_times),
        pack_nanoseconds_minimum: pack_minimum,
        verify_nanoseconds_median: median(&mut verify_times),
        verify_nanoseconds_minimum: verify_minimum,
    })
}

fn duration_nanoseconds(started: Instant) -> u64 {
    u64::try_from(started.elapsed().as_nanos()).unwrap_or(u64::MAX)
}

fn median(values: &mut [u64]) -> u64 {
    values.sort_unstable();
    let middle = values.len() / 2;
    if values.len() % 2 == 1 {
        values[middle]
    } else {
        let lower = values[middle - 1];
        let upper = values[middle];
        lower.midpoint(upper)
    }
}

#[cfg(test)]
mod tests {
    use super::{MAX_BENCHMARK_ITERATIONS, benchmark, median};
    use crate::{ContainerError, Profile};

    fn fixture() -> Vec<u8> {
        let mut bytes = vec![0u8; 16_384];
        bytes[..4].copy_from_slice(b"\x7fELF");
        bytes[4] = 2;
        bytes[5] = 1;
        bytes[6] = 1;
        bytes[16..18].copy_from_slice(&2u16.to_le_bytes());
        bytes[18..20].copy_from_slice(&62u16.to_le_bytes());
        bytes[20..24].copy_from_slice(&1u32.to_le_bytes());
        bytes[24..32].copy_from_slice(&0x40_1000u64.to_le_bytes());
        bytes[32..40].copy_from_slice(&64u64.to_le_bytes());
        bytes[52..54].copy_from_slice(&64u16.to_le_bytes());
        bytes[54..56].copy_from_slice(&56u16.to_le_bytes());
        bytes[56..58].copy_from_slice(&1u16.to_le_bytes());
        bytes[64..68].copy_from_slice(&1u32.to_le_bytes());
        bytes[72..80].copy_from_slice(&0u64.to_le_bytes());
        bytes[96..104].copy_from_slice(&16_384u64.to_le_bytes());
        bytes[104..112].copy_from_slice(&16_384u64.to_le_bytes());
        bytes[256..].fill(0x41);
        bytes
    }

    #[test]
    fn reports_every_profile_in_stable_order() {
        let report = benchmark(&fixture(), 0o755, 2).unwrap();
        assert_eq!(report.iterations, 2);
        assert_eq!(report.warmup_iterations, 1);
        assert_eq!(report.profiles.len(), 4);
        assert_eq!(report.profiles[0].profile, Profile::Fast);
        assert_eq!(report.profiles[1].profile, Profile::Balanced);
        assert_eq!(report.profiles[2].profile, Profile::Small);
        assert_eq!(report.profiles[3].profile, Profile::Auto);
        assert!(
            report
                .profiles
                .iter()
                .all(|profile| profile.pack_nanoseconds_median > 0)
        );
    }

    #[test]
    fn bounds_iteration_count() {
        assert!(matches!(
            benchmark(&fixture(), 0o755, 0),
            Err(ContainerError::InvalidIterations { .. })
        ));
        assert!(matches!(
            benchmark(&fixture(), 0o755, MAX_BENCHMARK_ITERATIONS + 1),
            Err(ContainerError::InvalidIterations { .. })
        ));
    }

    #[test]
    fn calculates_integer_medians_without_overflow() {
        assert_eq!(median(&mut [9]), 9);
        assert_eq!(median(&mut [9, 3, 6]), 6);
        assert_eq!(median(&mut [u64::MAX, u64::MAX]), u64::MAX);
        assert_eq!(median(&mut [2, 1]), 1);
    }
}
