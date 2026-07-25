//! Guards the README's sample `rexymcp.toml` against the config structs.
//!
//! The README is prose, not generated, so nothing else stops it drifting behind
//! a newly added knob — which is exactly how `[governor] read_only_stall_threshold`
//! went undocumented for two milestones (M34 bug-01-1). Lives in `tests/` rather
//! than beside `init.rs`'s own template tests so that editing the README rebuilds
//! only this target, not the `rexymcp` binary.

use rexymcp_executor::config::{GovernorConfig, ModelOverride};

const README: &str = include_str!("../../README.md");

/// Field names of a `#[derive(Serialize)]` struct, as they appear as TOML keys.
fn field_names<T: serde::Serialize>(value: &T) -> Vec<String> {
    serde_json::to_value(value)
        .expect("struct serializes to a JSON object")
        .as_object()
        .expect("struct serializes to a JSON object")
        .keys()
        .cloned()
        .collect()
}

/// The lines of the sample config's `[<header>…]` section: everything after the
/// header line up to the next TOML table header or the end of the code fence.
/// Scoping matters — every governor knob is *also* a per-model override key, so
/// an unscoped search would let either block alone satisfy both tests.
fn section<'a>(text: &'a str, header: &str) -> Vec<&'a str> {
    text.lines()
        .skip_while(|line| !line.trim_start().starts_with(header))
        .skip(1)
        .take_while(|line| {
            let rest = line.trim_start().trim_start_matches('#').trim_start();
            !rest.starts_with('[') && !rest.starts_with("```")
        })
        .collect()
}

/// Whether `lines` document `key` as a TOML assignment — tolerant of a leading
/// `#` and of the sample block's column alignment.
fn documents_key(lines: &[&str], key: &str) -> bool {
    lines.iter().any(|line| {
        let rest = line.trim_start().trim_start_matches('#').trim_start();
        rest.strip_prefix(key)
            .is_some_and(|tail| tail.trim_start().starts_with('='))
    })
}

#[test]
fn readme_documents_every_governor_knob() {
    let block = section(README, "[governor]");
    assert!(
        !block.is_empty(),
        "README must have a [governor] sample block"
    );
    for key in field_names(&GovernorConfig::default()) {
        assert!(
            documents_key(&block, &key),
            "README's [governor] sample block must document {key}"
        );
    }
}

#[test]
fn readme_documents_every_per_model_override() {
    let block = section(README, "[models.");
    assert!(
        !block.is_empty(),
        "README must have a [models] sample block"
    );
    for key in field_names(&ModelOverride::default()) {
        assert!(
            documents_key(&block, &key),
            "README's [models] sample block must document the {key} override"
        );
    }
}
