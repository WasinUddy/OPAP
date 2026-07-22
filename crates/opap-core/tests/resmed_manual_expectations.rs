// Copyright (C) 2011-2018 Mark Watkins
// Copyright (C) 2019-2026 The OSCAR Team
// Copyright (C) 2026 OPAP contributors
// SPDX-License-Identifier: GPL-3.0-only
//
// Adapted from OSCAR's ResMed fixture traversal and YAML-emission approach:
// https://gitlab.com/CrimsonNape/OSCAR-code
// Upstream commit: 64c5e90a26f91fb15868bcfcccde0c1e1522ac86
// Relevant upstream files: oscar/tests/resmedtests.cpp,
// oscar/tests/sessiontests.cpp
// This quarantined, ignored harness does not execute OSCAR and is not an independently
// verified differential oracle. Each private fixture manually supplies its
// expected OSCAR identity output, identifies the exact source revision used,
// and declares OPAP's intentional product-family normalization correction.
// Each fixture directory contains:
// - card/
// - expected/oscar-code-revision.txt
// - expected/opap-corrections.txt
// - expected/oscar-machine-info.json (manually supplied raw OSCAR expectation)
// - expected/opap-machine-info.json (expected output after declared corrections)
// Modified: 2026-07-23

use opap_core::resmed::{MachineInfo, detect_card, read_machine_info};
use std::env;
use std::fs;
use std::path::Path;

const OSCAR_CODE_REVISION: &str = "64c5e90a26f91fb15868bcfcccde0c1e1522ac86";
const OPAP_CORRECTIONS: &str = "product-family-normalization-v1";

#[test]
#[ignore = "requires a private or explicitly anonymized ResMed fixture corpus"]
fn checks_private_machine_info_expectations_with_pinned_provenance() {
    let root = env::var_os("OPAP_RESMED_FIXTURES")
        .expect("set OPAP_RESMED_FIXTURES to the ResMed conformance corpus");
    let root = Path::new(&root);
    let mut tested = 0usize;

    for entry in fs::read_dir(root).expect("read fixture root") {
        let case = entry.expect("read fixture case").path();
        let card = case.join("card");
        if !detect_card(&card) {
            continue;
        }

        let revision_path = case.join("expected/oscar-code-revision.txt");
        let revision = fs::read_to_string(&revision_path)
            .unwrap_or_else(|error| panic!("read {}: {error}", revision_path.display()));
        assert_eq!(
            revision.trim(),
            OSCAR_CODE_REVISION,
            "fixture {} declares another OSCAR-code revision",
            case.display()
        );

        let corrections_path = case.join("expected/opap-corrections.txt");
        let corrections = fs::read_to_string(&corrections_path)
            .unwrap_or_else(|error| panic!("read {}: {error}", corrections_path.display()));
        assert_eq!(
            corrections.trim(),
            OPAP_CORRECTIONS,
            "fixture {} does not declare the expected OPAP correction policy",
            case.display()
        );

        let oscar_path = case.join("expected/oscar-machine-info.json");
        let oscar_bytes = fs::read(&oscar_path)
            .unwrap_or_else(|error| panic!("read {}: {error}", oscar_path.display()));
        let oscar_json: serde_json::Value = serde_json::from_slice(&oscar_bytes)
            .unwrap_or_else(|error| panic!("parse {}: {error}", oscar_path.display()));
        assert!(
            oscar_json.get("source_model").is_none(),
            "raw OSCAR fixture must not contain OPAP-only source_model: {}",
            case.display()
        );
        let oscar: MachineInfo = serde_json::from_slice(&oscar_bytes)
            .unwrap_or_else(|error| panic!("parse {}: {error}", oscar_path.display()));
        let expected_path = case.join("expected/opap-machine-info.json");
        let expected_opap: MachineInfo = serde_json::from_slice(
            &fs::read(&expected_path)
                .unwrap_or_else(|error| panic!("read {}: {error}", expected_path.display())),
        )
        .unwrap_or_else(|error| panic!("parse {}: {error}", expected_path.display()));
        let actual = read_machine_info(&card)
            .unwrap_or_else(|error| panic!("parse {}: {error}", card.display()));

        assert_eq!(actual, expected_opap, "OPAP fixture {}", case.display());
        assert_eq!(actual.brand, oscar.brand, "brand {}", case.display());
        assert_eq!(
            actual.model_number,
            oscar.model_number,
            "product code {}",
            case.display()
        );
        assert_eq!(actual.serial, oscar.serial, "serial {}", case.display());
        tested += 1;
    }

    assert!(
        tested > 0,
        "no valid ResMed fixture cases found in {}",
        root.display()
    );
}
