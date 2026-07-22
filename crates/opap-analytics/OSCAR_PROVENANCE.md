# OSCAR analytics provenance

Reference repository: `CrimsonNape/OSCAR-SQL`
Pinned commit: `3741e5b423e4b5796c51a9d447e83b2525963d50`
License: GNU GPL v3

Line links below are immutable because they include that commit. The Rust crate
is a clean, typed port of the cited behavior; it does not compile or call C++.

The four tests in `tests/oscar_compat.rs` are source-derived, hand-auditable
examples; they are not a differential oracle. Release-level parity still
requires the repository-level compatibility harness to run the pinned C++ and
Rust implementations over identical synthetic/anonymized fixtures, record the
oracle revision/toolchain, and apply documented `float`-to-`f64` tolerances.

## Ported behavior

| OPAP behavior | Pinned OSCAR reference | Compatibility statement |
|---|---|---|
| A session with no slices uses `last - first`; otherwise only `MaskOn` slices contribute | [`session.h` lines 215-230](https://gitlab.com/CrimsonNape/OSCAR-SQL/-/blob/3741e5b423e4b5796c51a9d447e83b2525963d50/oscar/SleepLib/session.h#L215-230) | Same selection rule and sum for valid, non-overlapping slices. OPAP rejects overlapping slices as malformed instead of inheriting OSCAR's double-counting behavior. |
| Daily usage unions overlapping enabled ranges | [`day.cpp` lines 684-758](https://gitlab.com/CrimsonNape/OSCAR-SQL/-/blob/3741e5b423e4b5796c51a9d447e83b2525963d50/oscar/SleepLib/day.cpp#L684-758) | Same union-duration result for valid intervals. OPAP uses half-open intervals and checked `i128`/`u64` arithmetic. |
| AHI contains Clear Airway, All Apnea, Obstructive, Hypopnea, and Unclassified Apnea channels | [`schema.cpp` lines 415-425](https://gitlab.com/CrimsonNape/OSCAR-SQL/-/blob/3741e5b423e4b5796c51a9d447e83b2525963d50/oscar/SleepLib/schema.cpp#L415-425), with channel meanings at [`schema.cpp` lines 168-177](https://gitlab.com/CrimsonNape/OSCAR-SQL/-/blob/3741e5b423e4b5796c51a9d447e83b2525963d50/oscar/SleepLib/schema.cpp#L168-177) | Same five separately supplied counts. OPAP calls All Apnea `device_reported_apnea` to avoid confusing it with the checked total. |
| AHI is total contributing events divided by CPAP hours; RDI adds RERA | [`day.h` lines 249-263](https://gitlab.com/CrimsonNape/OSCAR-SQL/-/blob/3741e5b423e4b5796c51a9d447e83b2525963d50/oscar/SleepLib/day.h#L249-263) | Same formula for identical duration/count inputs. OPAP rejects zero duration rather than returning zero, so missing time cannot look like a measured zero. |
| Waveforms weight each value by `sample_count * rate`; step values use elapsed holding time | [`session.cpp` lines 1265-1304](https://gitlab.com/CrimsonNape/OSCAR-SQL/-/blob/3741e5b423e4b5796c51a9d447e83b2525963d50/oscar/SleepLib/session.cpp#L1265-1304) | OPAP makes represented milliseconds explicit for sampled waveforms. Exact waveform parity requires the importer to preserve OSCAR-equivalent value quantization and millisecond rate. OSCAR's step/event summaries use whole seconds; parity for those is not claimed by this waveform API. |
| Session/day percentiles sort value bins and use time-weighted ranks | [`session.cpp` lines 2357-2472](https://gitlab.com/CrimsonNape/OSCAR-SQL/-/blob/3741e5b423e4b5796c51a9d447e83b2525963d50/oscar/SleepLib/session.cpp#L2357-2472), [`day.cpp` lines 349-478](https://gitlab.com/CrimsonNape/OSCAR-SQL/-/blob/3741e5b423e4b5796c51a9d447e83b2525963d50/oscar/SleepLib/day.cpp#L349-478), value ordering at [`common.cpp` lines 398-401](https://gitlab.com/CrimsonNape/OSCAR-SQL/-/blob/3741e5b423e4b5796c51a9d447e83b2525963d50/oscar/SleepLib/common.cpp#L398-401) | Same rank walk and exact-boundary interpolation on ordinary interior cases. See guarded difference below. |
| Plain median averages the two middle values for an even-length sequence | [`common.h` lines 131-153](https://gitlab.com/CrimsonNape/OSCAR-SQL/-/blob/3741e5b423e4b5796c51a9d447e83b2525963d50/oscar/SleepLib/common.h#L131-153) | Equal-duration samples produce the same familiar results in the reliable interior examples (for example, 1/2/3/4 gives 2.5). |
| Generic session-to-day assignment compares corrected local start time with the configured split | [`machine.cpp` lines 359-375](https://gitlab.com/CrimsonNape/OSCAR-SQL/-/blob/3741e5b423e4b5796c51a9d447e83b2525963d50/oscar/SleepLib/machine.cpp#L359-375); default noon preference at [`profiles.cpp` lines 698-704](https://gitlab.com/CrimsonNape/OSCAR-SQL/-/blob/3741e5b423e4b5796c51a9d447e83b2525963d50/oscar/SleepLib/profiles.cpp#L698-704) | OPAP implements the noon case without ambient timezone state. Separate APIs accept either normalized UTC + applicable offset or raw device-local time + device correction, preventing correction from being applied twice. |

## Deliberate guards and parity unknowns

- In the pinned percentile code, an exact upper cumulative boundary can select
  `k + 1` after the last bin, while high-tail midpoint interpolation can
  extrapolate beyond the observed maximum. OPAP guards the final bin and clamps
  the interpolation factor. Parity is intentionally not claimed for those edge
  cases; OPAP guarantees a finite result within observed min/max.
- OSCAR optionally moves sessions when `combineCloseSessions` is enabled and has
  a ResMed-specific near-split adjustment ([`machine.cpp` lines 376-405](https://gitlab.com/CrimsonNape/OSCAR-SQL/-/blob/3741e5b423e4b5796c51a9d447e83b2525963d50/oscar/SleepLib/machine.cpp#L376-405)). The pure noon helper does not model those profile- and loader-dependent policies.
- OSCAR aggregates quantized raw integer bins multiplied by channel gain. OPAP
  accepts finite physical `f64` values. Differential fixtures must preserve the
  loader's quantization/gain boundary before claiming exact percentile parity.
- OSCAR's `EventDataType` is `float`; OPAP computes public summaries in `f64`.
  Formula-level examples are exact where representable, but general
  differential comparisons need a named floating-point tolerance.
- OSCAR's C++ often returns numeric zero for missing data. OPAP represents an
  absent signal with `None` and rejects events without therapy time. This avoids
  conflating “not measured” with a measured zero.
- These functions only summarize recorded device data. Neither OSCAR provenance
  nor numerical parity gives the output a medical interpretation.
