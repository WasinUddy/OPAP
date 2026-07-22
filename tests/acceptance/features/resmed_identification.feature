Feature: ResMed card detection and identification
  OPAP must preserve OSCAR-compatible card recognition and machine identity
  while keeping acceptance fixtures synthetic, temporary, and local.

  Scenario: Identify a valid legacy TGT card
    Given a synthetic directory with a valid ResMed card structure
    And legacy TGT identification is present
    When OPAP detects and identifies the card
    Then the card is detected as ResMed
    And the legacy machine identity is returned
    And the CLI and library results agree

  Scenario: Prefer JSON identification over legacy TGT identification
    Given a synthetic directory with a valid ResMed card structure
    And legacy TGT identification is present
    And JSON identification is present
    When OPAP detects and identifies the card
    Then the card is detected as ResMed
    And the JSON machine identity is returned
    And the CLI and library results agree

  Scenario: Reject a directory that is not a ResMed card
    Given an empty synthetic card directory
    When OPAP detects and identifies the card
    Then the card is not detected as ResMed
    And identification fails because the card is invalid
    And the CLI and library results agree

  Scenario: Report missing identification on an otherwise valid card
    Given a synthetic directory with a valid ResMed card structure
    When OPAP detects and identifies the card
    Then the card is detected as ResMed
    And identification fails because machine identity is missing
    And the CLI and library results agree

  Scenario: Keep synthetic card data private and disposable
    Given a synthetic directory with a valid ResMed card structure
    And legacy TGT identification is present
    And the original synthetic card contents are recorded
    When OPAP detects and identifies the card
    Then the fixture is outside the source repository
    And inspecting the card leaves its contents unchanged
    And destroying the fixture removes its synthetic data
