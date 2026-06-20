# lattice-registry

A local catalog of BFO-grounded ontologies: it fetches OWL files, records whether each one actually anchors to BFO and how many classes it has, and gives you a stable path to the cached copy.

## Why it exists

Before you can align two ontologies you have to have them, know they're real, and find them again. The OBO Foundry publishes hundreds; most of an alignment session is spent downloading files, re-downloading them, and squinting at each to decide whether it's BFO-grounded at all. `lattice-registry` does that bookkeeping once. It pulls an ontology, parses it to check the BFO grounding and count the classes, caches the file with its ETag so the next fetch is a no-op, and records the metadata in a catalog you can list and query. Tools downstream — `lattice-bridge` in particular — ask it for a path by name instead of carrying URLs around.

## Install

```
cargo install --path .
```

Requires Rust ≥ 1.85.

## Quickstart

Populate the catalog from the OBO Foundry registry, then inspect it:

```
lattice-registry sync                  # ingest the OBO Foundry registry
lattice-registry list                  # tabular view: ID, title, domain, class count, BFO?
lattice-registry list --domain quality # filter by domain
lattice-registry show bfo              # full metadata for one entry
lattice-registry path iao              # print the cached .owl path — for piping into other tools
```

`list` prints a table:

```
ID                   TITLE                          DOMAIN               CLASSES  BFO
------------------------------------------------------------------------------------------
bfo                  Basic Formal Ontology          upper                35       true
iao                  Information Artifact Ontology   information          ...      true
```

Add a single ontology by IRI or URL, outside the Foundry sync:

```
lattice-registry add http://purl.obolibrary.org/obo/ro.owl
```

`add` and `sync` are cache-aware: an unchanged ETag means no re-download. `path` exits non-zero (code 2) when the id isn't in the catalog, so it's safe to script.

## What the catalog records

Each entry carries `id`, `iri`, `title`, `domain`, `class_count`, `bfo_grounded`, `license`, the `cached_path`, the `etag`, and `last_fetched`. The BFO-grounding check and class count come from parsing the OWL with horned-owl; a file that fails to parse is still cataloged, with `bfo_grounded = false`, rather than dropped. The catalog lives at `~/.cache/lattice/registry/catalog.json` and the cached OWL files under `~/.cache/lattice/registry/owls/`.

Today `sync` supports one upstream registry: `obo-foundry`.

## The seed scripts

The binary catalogs single ontologies; the federation across them lives in two shell scripts in `scripts/`:

- `lattice-seed.sh` — ingest BFO, RO, IAO, COB, and SWO, bridge them pairwise against BFO/IAO (via `lattice-bridge`), and join the result into a federated store under `~/.cache/lattice/store/`. Idempotent: re-running skips already-cached ontologies.
- `lattice-cco-ingest.sh` — add the Common Core Ontologies (CCO v2.0) mid-layer to that store. CCO is BSD-3.1, attribution required.

## Where it fits

`lattice-registry` and [`lattice-bridge`](https://github.com/j0yen/lattice-bridge) are a pair. The registry is the catalog; the bridge does the alignment. `lattice-bridge align --from-registry --a iao --b cco` resolves both names through `lattice-registry path`.

## License

MIT
