# OSCAR analytics source provenance (not parity)

Reference repository:
[`CrimsonNape/OSCAR-code`](https://gitlab.com/CrimsonNape/OSCAR-code)

Pinned commit: `64c5e90a26f91fb15868bcfcccde0c1e1522ac86`

License: GNU GPL v3

Every OSCAR link below names that immutable commit. This crate does not compile,
load, or execute OSCAR's C++; it implements small Rust calculations derived
from inspected source. The tests in `tests/source_derived_examples.rs` are
hand-worked examples of those calculations. They are **not** compatibility
tests and are **not** a differential oracle.

## Source-to-Rust map

| OPAP calculation | Exact OSCAR source | What is and is not reproduced |
|---|---|---|
| Select a complete session when it has no slices; otherwise select only `MaskOn` slices | [`Session::hours`, `session.h` lines 188-205](https://gitlab.com/CrimsonNape/OSCAR-code/-/blob/64c5e90a26f91fb15868bcfcccde0c1e1522ac86/oscar/SleepLib/session.h#L188-205) | `summarize_session_usage` uses the same selection rule for valid inputs. It rejects reversed, out-of-window, or overlapping slices instead of accepting and summing them. |
| Union overlapping CPAP ranges for daily therapy time | [`Day::total_time(MachineType)`, `day.cpp` lines 757-830](https://gitlab.com/CrimsonNape/OSCAR-code/-/blob/64c5e90a26f91fb15868bcfcccde0c1e1522ac86/oscar/SleepLib/day.cpp#L757-830), called by [`Day::hours(MachineType)`, `day.h` lines 178-183](https://gitlab.com/CrimsonNape/OSCAR-code/-/blob/64c5e90a26f91fb15868bcfcccde0c1e1522ac86/oscar/SleepLib/day.h#L178-183) | `summarize_therapy_usage` produces the union duration for already-selected, valid intervals. OSCAR selects enabled sessions of the requested machine type and ignores non-positive unsliced ranges. The raw OPAP function has no enabled or machine-type field; `aggregate_by_noon` filters disabled sessions but still relies on its caller to supply CPAP-only inputs. Broad `Day::total_time(MT_CPAP)` parity is therefore not claimed. |
| Choose the five channels contributing to AHI | [`schema.cpp` lines 413-424](https://gitlab.com/CrimsonNape/OSCAR-code/-/blob/64c5e90a26f91fb15868bcfcccde0c1e1522ac86/oscar/SleepLib/schema.cpp#L413-424), with channel meanings at [`schema.cpp` lines 167-180](https://gitlab.com/CrimsonNape/OSCAR-code/-/blob/64c5e90a26f91fb15868bcfcccde0c1e1522ac86/oscar/SleepLib/schema.cpp#L167-180) | `AhiEventCounts` exposes Clear Airway, All Apnea, Obstructive, Hypopnea, and Unclassified Apnea separately. OPAP names All Apnea `device_reported_apnea` so it is not mistaken for the computed total. |
| Divide AHI event count by CPAP hours; add RERA for RDI | [`Day::calcAHI` and `Day::calcRDI`, `day.h` lines 249-263](https://gitlab.com/CrimsonNape/OSCAR-code/-/blob/64c5e90a26f91fb15868bcfcccde0c1e1522ac86/oscar/SleepLib/day.h#L249-263) | `calculate_event_indices` uses the same algebraic formula for identical positive duration and count inputs. OPAP rejects zero therapy time and checked-count overflow; OSCAR's inline division has no zero-time guard. Formula equivalence is not end-to-end parity because session selection can differ. |
| Build value weights for waveform and step/event summaries | [`Session::updateCountSummary`, `session.cpp` lines 1083-1210](https://gitlab.com/CrimsonNape/OSCAR-code/-/blob/64c5e90a26f91fb15868bcfcccde0c1e1522ac86/oscar/SleepLib/session.cpp#L1083-1210) | For one waveform list, OSCAR adds `sample_count * rate` to each raw value bin. Across multiple waveform lists, `valsum` remains cumulative and every list re-adds all counts seen so far using the current list's rate. The event path uses whole elapsed seconds and omits a trailing hold. OPAP accepts explicit positive milliseconds per physical value and reproduces none of those multi-list/truncation behaviors. Importers must construct weights deliberately. |
| Compute a time-weighted day percentile from sorted value bins | [`Day::percentile`, `day.cpp` lines 345-474](https://gitlab.com/CrimsonNape/OSCAR-code/-/blob/64c5e90a26f91fb15868bcfcccde0c1e1522ac86/oscar/SleepLib/day.cpp#L345-474), with value ordering at [`common.cpp` lines 369-372](https://gitlab.com/CrimsonNape/OSCAR-code/-/blob/64c5e90a26f91fb15868bcfcccde0c1e1522ac86/oscar/SleepLib/common.cpp#L369-372) | `weighted_percentile` follows the source-derived rank walk and boundary interpolation for ordinary positive, interior inputs after equivalent values and weights have been supplied. OPAP bounds all results to the observed range and validates inputs, so exact C++ edge behavior is intentionally not copied. |
| Compute a session percentile | [`Session::percentile`, `session.cpp` lines 2239-2295](https://gitlab.com/CrimsonNape/OSCAR-code/-/blob/64c5e90a26f91fb15868bcfcccde0c1e1522ac86/oscar/SleepLib/session.cpp#L2239-2295) | This is **not** the algorithm implemented by `weighted_percentile`. For one event list, OSCAR's session method applies unweighted `nth_element` with no averaging: `[1, 2, 3, 4]` at 50% selects the third order statistic (`3`), while OPAP's time-weighted day-style calculation returns `2.5` for equal weights. Across multiple event lists, OSCAR retains the last list's `cnt` and resets the destination pointer for each copy; OPAP does not reproduce that behavior. |
| Compute the ordinary median of an in-memory sequence | [`median`, `common.h` lines 111-134](https://gitlab.com/CrimsonNape/OSCAR-code/-/blob/64c5e90a26f91fb15868bcfcccde0c1e1522ac86/oscar/SleepLib/common.h#L111-134) | OSCAR's standalone helper averages the two middle values for an even sequence. The source-derived equal-weight example also returns `2.5` in OPAP, but OPAP reaches it through the day-style weighted algorithm, not this helper or `Session::percentile`. |
| Combine session means into a day mean | [`Day::wavg`, `day.cpp` lines 656-678](https://gitlab.com/CrimsonNape/OSCAR-code/-/blob/64c5e90a26f91fb15868bcfcccde0c1e1522ac86/oscar/SleepLib/day.cpp#L656-678) | OSCAR weights each session mean by the complete session length. OPAP combines the explicit represented duration of every supplied sample. Results differ when signal coverage is not proportional to full session duration, so day-mean parity is not claimed. |
| Assign a session to the preceding date before the configured split | [`Machine::pickDate`, `machine.cpp` lines 257-285](https://gitlab.com/CrimsonNape/OSCAR-code/-/blob/64c5e90a26f91fb15868bcfcccde0c1e1522ac86/oscar/SleepLib/machine.cpp#L257-285), default split in [`profiles.h` lines 705-723](https://gitlab.com/CrimsonNape/OSCAR-code/-/blob/64c5e90a26f91fb15868bcfcccde0c1e1522ac86/oscar/SleepLib/profiles.h#L705-723), and forced ResMed settings in [`profiles.cpp` lines 372-381](https://gitlab.com/CrimsonNape/OSCAR-code/-/blob/64c5e90a26f91fb15868bcfcccde0c1e1522ac86/oscar/SleepLib/profiles.cpp#L372-381) | OPAP implements only the fixed-noon comparison. It does not reproduce arbitrary profile split times or nearby-session combining. It accepts corrected naive-local time, or normalized UTC plus an explicitly applicable offset, instead of reading ambient host timezone state. |

## Deliberate guards and exact differences

- OSCAR's day-percentile interpolation uses `floor(total_weight * p)`. At an
  exact final cumulative boundary it can access `k + 1` after the last bin. At
  other boundaries, a percentile beyond the next bin's midpoint can
  extrapolate past that next observed value. OPAP returns the final bin when no
  successor exists and clamps both the interpolation factor and result. Its
  percentile is finite and bounded by observed min/max; those edge results are
  intentionally different.
- OSCAR aggregates quantized `EventStoreType` bins and multiplies them by one
  `float` gain; its day code explicitly assumes gains do not change across the
  day's sessions ([`day.cpp` lines 363-385](https://gitlab.com/CrimsonNape/OSCAR-code/-/blob/64c5e90a26f91fb15868bcfcccde0c1e1522ac86/oscar/SleepLib/day.cpp#L363-385)
  and [`day.cpp` lines 418-420](https://gitlab.com/CrimsonNape/OSCAR-code/-/blob/64c5e90a26f91fb15868bcfcccde0c1e1522ac86/oscar/SleepLib/day.cpp#L418-420)).
  [`EventDataType` is `float`](https://gitlab.com/CrimsonNape/OSCAR-code/-/blob/64c5e90a26f91fb15868bcfcccde0c1e1522ac86/oscar/SleepLib/common.h#L61-74),
  while OPAP accepts finite physical `f64` values. A future differential
  comparison must preserve quantization/gain boundaries and define a float
  tolerance.
- OSCAR commonly returns numeric zero for absent inputs, and its AHI/RDI
  division can produce non-finite output when CPAP time is zero. OPAP represents
  an absent signal with `None`, rejects an empty non-optional signal, and
  rejects events without positive therapy time.
- OPAP uses checked `i128`/`u64` duration arithmetic and checked event-count
  addition. It rejects reversed intervals, slices outside their session, and
  overlapping non-empty slices. OSCAR's cited paths do not apply equivalent
  guards. OPAP also handles intervals beginning at timestamp zero; OSCAR's
  union implementation uses zero as an internal “not started” sentinel.
- OSCAR applies CPAP clock drift in [`Session::first`/`last`, `session.cpp`
  lines 2429-2448](https://gitlab.com/CrimsonNape/OSCAR-code/-/blob/64c5e90a26f91fb15868bcfcccde0c1e1522ac86/oscar/SleepLib/session.cpp#L2429-2448),
  then converts an epoch instant through Qt's local-time behavior. OPAP has no
  timezone database and never guesses daylight-saving transitions. Its
  normalized-UTC API requires the caller to provide the offset that applied at
  that instant; its raw-device API requires an explicit clock correction.
- OSCAR can move sessions across the split when `combineCloseSessions` is
  enabled and contains a ResMed-specific near-split adjustment in
  [`Machine::AddSession`, `machine.cpp` lines 334-399](https://gitlab.com/CrimsonNape/OSCAR-code/-/blob/64c5e90a26f91fb15868bcfcccde0c1e1522ac86/oscar/SleepLib/machine.cpp#L334-399).
  OPAP's pure fixed-noon helper models neither policy.

## ResMed integration gate

The core index already carries `ResmedSessionCandidate.resmed_day`, derived
from the card's filename-local therapy-day contract, while a candidate's
selected EDF timestamp can differ and may produce a drift warning. A real
ResMed integration must carry that authoritative candidate day into analytics;
it must not recompute the day from a header-selected timestamp that can cross
noon. `aggregate_by_noon` remains suitable only when its supplied corrected
local start is the authoritative day source. An API that accepts a preassigned
therapy day is required before wiring drifted real imports to this aggregator.

## What a differential oracle would require

A genuine oracle must build and run the pinned C++ commit and this Rust crate
over identical synthetic or anonymized fixtures. It must record the OSCAR
revision, compiler, Qt/platform timezone behavior, profile settings, session
enable/type selection, gain/quantization conversion, and named float
tolerances. Known guarded differences must be asserted separately rather than
silently accepted as “close enough.” None of the current crate tests does this,
and this document makes no claim of whole-session, whole-day, or medical parity.
