Feature: Privacy-safe application service workflow
  OPAP exposes only honest, durable workflow states while real session import
  remains unavailable. Every source in these scenarios is synthetic and temporary.

  Scenario: Bootstrap reports that session import is unavailable
    Given a fresh local OPAP service database
    When the renderer requests application bootstrap
    Then bootstrap reports session import is unavailable

  Scenario: Inspect a supported source without exposing private native details
    Given a fresh local OPAP service database
    And a synthetic supported ResMed source with a full serial
    When the native service inspects the source
    Then the inspection returns an opaque source handle and only a serial suffix
    And the renderer inspection JSON contains no absolute path or full serial

  Scenario: Redact privacy canaries embedded in device identity fields
    Given a fresh local OPAP service database
    And a synthetic supported ResMed source with a full serial
    And its device identity fields contain privacy canaries
    When the native service inspects the source
    Then the inspection returns an opaque source handle and only a serial suffix
    And the renderer inspection JSON contains no absolute path or full serial

  Scenario: Prepare one durable blocked job idempotently
    Given a fresh local OPAP service database
    And a synthetic supported ResMed source with a full serial
    When the same supported source import is prepared twice
    Then exactly one durable blocked job exists and no import ran
    And the repeated preparation returns the same job without creating one
    And the renderer cannot supply an import request key
    And the renderer job JSON contains no absolute path or full serial

  Scenario: Persist cancellation across application restart
    Given a fresh local OPAP service database
    And a synthetic supported ResMed source with a full serial
    When a prepared job is cancelled and the service is reopened
    Then the reopened job has the typed cancelled state

  Scenario: Recover an interrupted running job honestly
    Given a durable running job from an interrupted process
    When the OPAP service starts after the interruption
    Then the running job is recovered to blocked
    And untrusted legacy importer metadata is redacted

  Scenario: Refuse to prepare an unsupported source
    Given a fresh local OPAP service database
    And a synthetic unsupported source directory
    When the unsupported source import is prepared
    Then preparation fails with source not supported and creates no job
    And the renderer error JSON contains no absolute path

  Scenario: Sanitize a native source read failure
    Given a fresh local OPAP service database
    When a missing private native source is inspected
    Then inspection fails with a sanitized source unavailable error
    And the renderer error JSON contains no absolute path
