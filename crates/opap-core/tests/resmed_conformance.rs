// Copyright (C) 2011-2018 Mark Watkins
// Copyright (C) 2019-2026 The OSCAR Team
// Copyright (C) 2026 OPAP contributors
// SPDX-License-Identifier: GPL-3.0-only
//
// Ported and modified from the OSCAR-SQL ResMed conformance-test approach:
// https://gitlab.com/CrimsonNape/OSCAR-SQL
// Upstream commit: 3741e5b423e4b5796c51a9d447e83b2525963d50
// Relevant upstream files: oscar/tests/resmedtests.cpp,
// oscar/tests/sessiontests.cpp
// Modified: 2026-07-22

use opap_core::resmed::{MachineInfo, detect_card, read_machine_info};
use std::env;
use std::fs;
use std::path::Path;

#[test]
#[ignore = "requires a private or explicitly anonymized ResMed fixture corpus"]
fn matches_oscar_machine_info_goldens() {
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

        let expected_path = case.join("expected/machine-info.json");
        let expected: MachineInfo = serde_json::from_slice(
            &fs::read(&expected_path)
                .unwrap_or_else(|error| panic!("read {}: {error}", expected_path.display())),
        )
        .unwrap_or_else(|error| panic!("parse {}: {error}", expected_path.display()));
        let actual = read_machine_info(&card)
            .unwrap_or_else(|error| panic!("parse {}: {error}", card.display()));

        assert_eq!(actual, expected, "fixture {}", case.display());
        tested += 1;
    }

    assert!(
        tested > 0,
        "no valid ResMed fixture cases found in {}",
        root.display()
    );
}
