Feature: Partial ResMed BRP session import
  OPAP imports bounded waveform detail from synthetic ResMed BRP files without
  exposing source identity or mutating the user-selected card.

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
