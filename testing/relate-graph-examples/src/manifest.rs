//! Dataset manifest (ADR-0144): the neo4j-graph-examples datasets (core 5
//! plus the second and third waves), each mapped to its 5.0-line dump and
//! its walkthrough adoc. Adding a dataset is a row here, not code.

// The third wave (2026-10-10): seven more sandbox datasets with a
// `data/<name>-50.dump` and a walkthrough adoc. Left out: airport-routes
// (its walkthrough is GDS-centric and it has no `-50` dump) and star-wars
// (no dump — its data is CSVs plus a Neo4j Importer model, a different
// loading path).

/// One neo4j-graph-examples dataset.
#[derive(Debug, Clone)]
pub struct Dataset {
    /// Use-case name; also the Neo4j database name and the pulled
    /// relate directory name.
    pub name: &'static str,
    /// `data/<name>-50.dump` download URL (GitHub raw).
    pub dump_url: &'static str,
    /// sha256 of the dump, pinned so runs stay reproducible.
    pub dump_sha256: &'static str,
    /// `documentation/<name>.adoc` download URL (GitHub raw).
    pub adoc_url: &'static str,
    /// Env flag that must be `1` for the dataset to run by default: the
    /// heavy datasets (a 465 MB dump, tens of seconds of pull and load)
    /// delay the whole suite, so they run only on explicit opt-in or when
    /// named in `RELATE_GRAPH_EXAMPLES_DATASETS`. `None` — every
    /// dataset but twitch — always runs.
    pub opt_in_env: Option<&'static str>,
}

/// The sandbox datasets (ADR-0144; core 5 plus the second and third waves).
pub const DATASETS: &[Dataset] = &[
    Dataset {
        name: "movies",
        dump_url: "https://raw.githubusercontent.com/neo4j-graph-examples/movies/main/data/movies-50.dump",
        dump_sha256: "806ea150f5ce7387173e4ba54a8e0054f282e2f2021d4245117807744933a886",
        adoc_url: "https://raw.githubusercontent.com/neo4j-graph-examples/movies/main/documentation/movies.adoc",
        opt_in_env: None,
    },
    Dataset {
        name: "recommendations",
        dump_url: "https://raw.githubusercontent.com/neo4j-graph-examples/recommendations/main/data/recommendations-50.dump",
        dump_sha256: "c22134f5ef8e8239856003134bf746d77b705bc538e40bd7c1fa034673453734",
        adoc_url: "https://raw.githubusercontent.com/neo4j-graph-examples/recommendations/main/documentation/recommendations.adoc",
        opt_in_env: None,
    },
    Dataset {
        name: "northwind",
        dump_url: "https://raw.githubusercontent.com/neo4j-graph-examples/northwind/main/data/northwind-50.dump",
        dump_sha256: "3aef3fdf2f9e6a7e6e813d7fcd4e46a8ea7e75573ee7d4fb84f6884ec1f66b9f",
        adoc_url: "https://raw.githubusercontent.com/neo4j-graph-examples/northwind/main/documentation/northwind-old.adoc",
        opt_in_env: None,
    },
    Dataset {
        name: "fraud-detection",
        dump_url: "https://raw.githubusercontent.com/neo4j-graph-examples/fraud-detection/main/data/fraud-detection-50.dump",
        dump_sha256: "828d1cd1a3b8b20b4ee56e48533ca1feba8c7f3c144b4be39bdbb73b2c87c1bb",
        adoc_url: "https://raw.githubusercontent.com/neo4j-graph-examples/fraud-detection/main/documentation/fraud-detection.adoc",
        opt_in_env: None,
    },
    Dataset {
        name: "pole",
        dump_url: "https://raw.githubusercontent.com/neo4j-graph-examples/pole/main/data/pole-50.dump",
        dump_sha256: "f370895b222e1feb51321f731c0e5eb0fdb5824285d502944df8a48c4d4892aa",
        adoc_url: "https://raw.githubusercontent.com/neo4j-graph-examples/pole/main/documentation/pole.adoc",
        opt_in_env: None,
    },
    Dataset {
        name: "stackoverflow",
        dump_url: "https://raw.githubusercontent.com/neo4j-graph-examples/stackoverflow/main/data/stackoverflow-50.dump",
        dump_sha256: "bb584dca57b6299a24799ae4fce64465fa1f49e7525578804c778f8b24a73c05",
        adoc_url: "https://raw.githubusercontent.com/neo4j-graph-examples/stackoverflow/main/documentation/stackoverflow.adoc",
        opt_in_env: None,
    },
    Dataset {
        name: "twitter-v2",
        dump_url: "https://raw.githubusercontent.com/neo4j-graph-examples/twitter-v2/main/data/twitter-v2-50.dump",
        dump_sha256: "014b538cfb1ccc9f946284f4a5a07df17594bcf2a50051984399f37371ed05aa",
        adoc_url: "https://raw.githubusercontent.com/neo4j-graph-examples/twitter-v2/main/documentation/twitter.adoc",
        opt_in_env: None,
    },
    Dataset {
        name: "cybersecurity",
        dump_url: "https://raw.githubusercontent.com/neo4j-graph-examples/cybersecurity/main/data/cyber-security-ad-50.dump",
        dump_sha256: "520cc267cc8cbf568f587e36bbb1d7b9cacabb16982ebd2974ead2fbfe71b1d8",
        adoc_url: "https://raw.githubusercontent.com/neo4j-graph-examples/cybersecurity/main/documentation/cybersecurity.adoc",
        opt_in_env: None,
    },
    Dataset {
        name: "healthcare-analytics",
        dump_url: "https://raw.githubusercontent.com/neo4j-graph-examples/healthcare-analytics/main/data/healthcare-analytics-50.dump",
        dump_sha256: "fed00ade454d16e7c360adb95f75fe104085bafaced84b6e52847db42204a57c",
        adoc_url: "https://raw.githubusercontent.com/neo4j-graph-examples/healthcare-analytics/main/documentation/healthcare-analytics.adoc",
        opt_in_env: None,
    },
    Dataset {
        name: "fincen",
        dump_url: "https://raw.githubusercontent.com/neo4j-graph-examples/fincen/main/data/fincen-50.dump",
        dump_sha256: "e0e8f0a5616ef50fa9407a1ee02138f027df6e4a1c2c6d3585e25e88119016b8",
        adoc_url: "https://raw.githubusercontent.com/neo4j-graph-examples/fincen/main/documentation/fincen.adoc",
        opt_in_env: None,
    },
    // Third wave (2026-10-10) — same dump + adoc shape as the waves above.
    Dataset {
        name: "contact-tracing",
        dump_url: "https://raw.githubusercontent.com/neo4j-graph-examples/contact-tracing/main/data/contact-tracing-50.dump",
        dump_sha256: "d022c3a6cf4a3fa8516688dc24d10b0e668f942eab38bc82aca3d62a668e939e",
        adoc_url: "https://raw.githubusercontent.com/neo4j-graph-examples/contact-tracing/main/documentation/contact-tracing.adoc",
        opt_in_env: None,
    },
    Dataset {
        name: "entity-resolution",
        dump_url: "https://raw.githubusercontent.com/neo4j-graph-examples/entity-resolution/main/data/entity-resolution-50.dump",
        dump_sha256: "d9e8d19cb4bb44126c9fce25f6e4ccc4d93fb8d91effbdd56a0c8b52c72d669e",
        adoc_url: "https://raw.githubusercontent.com/neo4j-graph-examples/entity-resolution/main/documentation/entity-resolution.adoc",
        opt_in_env: None,
    },
    Dataset {
        name: "legis-graph",
        dump_url: "https://raw.githubusercontent.com/neo4j-graph-examples/legis-graph/main/data/legis-graph-50.dump",
        dump_sha256: "0309e2285b64e65791189910f6b665aa6807e2c19764275d8e7d3661cc936a39",
        adoc_url: "https://raw.githubusercontent.com/neo4j-graph-examples/legis-graph/main/documentation/legis-graph.adoc",
        opt_in_env: None,
    },
    Dataset {
        name: "network-management",
        dump_url: "https://raw.githubusercontent.com/neo4j-graph-examples/network-management/main/data/network-management-50.dump",
        dump_sha256: "0c54b653a56396191f37a2e0750a52abef649ad542ca6d73802496e46d30ed60",
        adoc_url: "https://raw.githubusercontent.com/neo4j-graph-examples/network-management/main/documentation/network-management.adoc",
        opt_in_env: None,
    },
    Dataset {
        name: "twitch",
        // 465 MB via Git LFS: the raw URL is a 134-byte LFS pointer, so the
        // download goes through the LFS media host.
        dump_url: "https://media.githubusercontent.com/media/neo4j-graph-examples/twitch/main/data/twitch-50.dump",
        dump_sha256: "bb0eed2e8901158db2472d2cbe3a9b5ffc6388b934496563cdadd44a82f26631",
        adoc_url: "https://raw.githubusercontent.com/neo4j-graph-examples/twitch/main/documentation/twitch.adoc",
        opt_in_env: Some("RELATE_GRAPH_EXAMPLES_TWITCH"),
    },
    Dataset {
        name: "twitter-trolls",
        dump_url: "https://raw.githubusercontent.com/neo4j-graph-examples/twitter-trolls/main/data/twitter-trolls-50.dump",
        dump_sha256: "0e9a0501cb7e0d2cdbd71174c2997e78e59ee491a00c4cb7c8b35940a2f83cfd",
        adoc_url: "https://raw.githubusercontent.com/neo4j-graph-examples/twitter-trolls/main/documentation/twitter-trolls.adoc",
        opt_in_env: None,
    },
    Dataset {
        name: "openstreetmap",
        dump_url: "https://raw.githubusercontent.com/neo4j-graph-examples/openstreetmap/main/data/openstreetmap-50.dump",
        dump_sha256: "6f5267bc8c4bdd4018c3842959dd449de9de427f9ccdc6c37242f36125aa1d25",
        adoc_url: "https://raw.githubusercontent.com/neo4j-graph-examples/openstreetmap/main/documentation/openstreetmap.adoc",
        opt_in_env: None,
    },
];

impl Dataset {
    pub fn find(name: &str) -> Option<&'static Dataset> {
        DATASETS.iter().find(|d| d.name == name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_names_are_unique_and_nonempty() {
        let mut seen = std::collections::HashSet::new();
        for d in DATASETS {
            assert!(!d.name.is_empty());
            assert!(seen.insert(d.name), "duplicate dataset {}", d.name);
            assert!(d.dump_url.starts_with("https://"), "{} dump url", d.name);
            assert!(d.adoc_url.starts_with("https://"), "{} adoc url", d.name);
            assert!(d.dump_url.ends_with("-50.dump"), "{} dump artifact", d.name);
            let sha = d.dump_sha256;
            assert!(
                sha.len() == 64 && sha.chars().all(|c| c.is_ascii_hexdigit()),
                "{} dump sha256 must be pinned (64 hex chars): {sha}",
                d.name
            );
        }
    }
}
