Feature: ResMed STR boundaries and bounded BRP session import
  OPAP imports source-selected serial-verified STR therapy intervals, retains
  provenance for bounded repairs, and associates bounded synthetic BRP detail
  without exposing source identity or mutating the user-selected card.

  Scenario: Import a verified STR interval without detail files
    Given a temporary ResMed card with one verified STR-only interval
    And the generated card contents are recorded
    When the BRP card is imported with an explicit fixed-offset clock
    Then exactly one STR-only summary session has an exact one-hour MaskOn slice
    And its imported identifiers are opaque and private
    And the generated card is unchanged and disposable

  Scenario: Preserve provenance for a bounded historical STR repair
    Given a temporary ResMed card with one historical STR interval missing mask-off
    When the BRP card is imported with an explicit fixed-offset clock
    Then the bounded STR repair is preserved as a session-scoped warning

  Scenario: Import exact STR usage with matching BRP detail
    Given a temporary ResMed card with verified STR and matching BRP detail
    And the generated card contents are recorded
    When the BRP card is imported with an explicit fixed-offset clock
    Then the STR plus BRP session keeps exact STR usage and bounded waveform detail
    And its imported identifiers are opaque and private
    And the generated card is unchanged and disposable

  Scenario: Keep direct ownership for two sessions on one ResMed day
    Given a temporary ResMed card with two verified STR intervals and two matching BRP files
    And the generated card contents are recorded
    When the BRP card is imported with an explicit fixed-offset clock
    Then each STR interval owns exactly one BRP file without cross-session expansion
    And the generated card is unchanged and disposable

  Scenario: Keep identity stable when only mask-off changes
    Given a temporary ResMed card with verified STR and matching BRP detail
    When the BRP card is imported with an explicit fixed-offset clock
    And the STR mask-off is extended by one hour and the card is reimported
    Then the changed mask-off updates exact usage without changing session identity

  Scenario: Preserve BRP fallback identity when STR serial verification fails
    Given a temporary ResMed card with a matching synthetic BRP recording
    When the BRP fallback is imported before and after installing a serial-mismatched STR
    Then the serial-mismatched STR preserves the exact BRP fallback session

  Scenario: Import calibrated flow with explicit device-clock provenance
    Given a temporary ResMed card with a matching synthetic BRP recording
    And the generated card contents are recorded
    When the BRP card is imported with an explicit fixed-offset clock
    Then exactly one partial BRP session is returned
    And its device clock is normalized with the exact offset and correction
    And its flow samples use the full EDF affine calibration in litres per minute
    And its imported identifiers are opaque and private
    And the partial-session limitation is reported
    And the generated card is unchanged and disposable

  Scenario: Reject a BRP recording from a different machine
    Given a temporary ResMed card with a mismatched synthetic BRP recording
    And the generated card contents are recorded
    When the BRP card is imported with an explicit fixed-offset clock
    Then no phantom session is returned
    And a privacy-safe BRP serial-mismatch warning is reported
    And the generated card is unchanged and disposable

  Scenario: Attach gapped SAD oximetry without inflating therapy usage
    Given a temporary ResMed card with matching synthetic BRP and gapped SAD recordings
    And the generated card contents are recorded
    When the BRP card is imported with an explicit fixed-offset clock
    Then exactly one partial BRP session is returned
    And its device clock is normalized with the exact offset and correction
    And its SAD missing sentinels split pulse into contiguous calibrated segments
    And its imported identifiers are opaque and private
    And the generated card is unchanged and disposable
