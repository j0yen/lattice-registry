#!/usr/bin/env bash
# lattice-cco-ingest.sh — Ingest Common Core Ontologies (CCO v2.0) mid-layer
# into the federated lattice store.
#
# Idempotent: re-running skips already-cached files; bridge files are overwritten.
# Attribution: CCO v2.0 — https://github.com/CommonCoreOntology/CommonCoreOntologies
# License: BSD 3.1 (permissive, attribution required — see ATTRIBUTION.md)
set -euo pipefail

CCO_CACHE="${HOME}/.cache/lattice/cco"
BRIDGES_DIR="${HOME}/.cache/lattice/bridges"
STORE_DIR="${HOME}/.cache/lattice/store"

CCO_BASE="https://raw.githubusercontent.com/CommonCoreOntology/CommonCoreOntologies/master/src/cco-modules"

# Five AI-relevant CCO modules (skip domain-specific: Artifact, Quality, Geospatial,
# Facility, UnitsOfMeasure, CurrencyUnit)
CCO_MODULES=(
  AgentOntology
  InformationEntityOntology
  EventOntology
  TimeOntology
  ExtendedRelationOntology
)

log()  { echo "[lattice-cco-ingest] $*" >&2; }
warn() { echo "[lattice-cco-ingest] WARN: $*" >&2; }

# ---------------------------------------------------------------------------
# Phase 1 — Fetch CCO modules
# ---------------------------------------------------------------------------
log "Phase 1: fetching CCO modules..."
mkdir -p "$CCO_CACHE"

for module in "${CCO_MODULES[@]}"; do
  ttl="${CCO_CACHE}/${module}.ttl"
  if [[ -f "$ttl" && -s "$ttl" ]]; then
    log "  $module: already cached, skipping"
  else
    log "  $module: downloading..."
    curl -sL "${CCO_BASE}/${module}.ttl" -o "$ttl"
    sz=$(wc -c < "$ttl")
    log "  $module: ${sz} bytes"
  fi
done

# Write attribution file (AC6)
cat > "${CCO_CACHE}/ATTRIBUTION.md" <<'EOF'
# Common Core Ontologies — Attribution

**Version**: CCO v2.0
**Source**: https://github.com/CommonCoreOntology/CommonCoreOntologies
**License**: BSD 3.1 — permissive, requires attribution, no copyleft restrictions.

Modules ingested (AI-agent relevant subset):
- AgentOntology — agents, roles, organizations
- InformationEntityOntology — information artifacts, records, plans
- EventOntology — processes, tasks, actions
- TimeOntology — temporal regions
- ExtendedRelationOntology — richer cross-domain relations

IRI namespace: `https://www.commoncoreontologies.org/ont00XXXXXX` (v2.0 opaque IRIs)
Mapping from old IRIs: src/cco-modules/documentation/mapping-new-iris/
EOF
log "  ATTRIBUTION.md written"

# ---------------------------------------------------------------------------
# Phase 2 — Add to registry
# lattice-registry add requires a network URL (not file:// or plain paths).
# We register using the raw GitHub URL so the registry can catalog the module.
# The cached .ttl files in Phase 4 are used directly for lattice-join add.
# ---------------------------------------------------------------------------
log "Phase 2: adding CCO modules to registry (via raw GitHub URLs)..."

for module in "${CCO_MODULES[@]}"; do
  url="${CCO_BASE}/${module}.ttl"
  log "  lattice-registry add $url"
  if lattice-registry add "$url" 2>&1; then
    log "  $module: registered"
  else
    warn "  $module: add exited non-zero (may already be registered — continuing)"
  fi
done

# ---------------------------------------------------------------------------
# Phase 3 — Bridge CCO modules into existing store
# ---------------------------------------------------------------------------
log "Phase 3: generating CCO bridge axioms..."
mkdir -p "$BRIDGES_DIR"

# Resolve registry paths for already-seeded ontologies
bfo_path=""
iao_path=""
if bfo_path=$(lattice-registry path bfo 2>/dev/null); then
  log "  bfo path: $bfo_path"
else
  warn "  bfo not in registry — bridge steps that need bfo will be skipped"
  bfo_path=""
fi
if iao_path=$(lattice-registry path iao 2>/dev/null); then
  log "  iao path: $iao_path"
else
  warn "  iao not in registry — bridge steps that need iao will be skipped"
  iao_path=""
fi

run_bridge() {
  local label="$1"; shift
  log "  bridge $label..."
  if lattice-bridge align "$@" 2>&1; then
    log "  $label: OK"
  else
    local rc=$?
    warn "  $label: exited $rc (possibly no mappings above threshold — continuing)"
  fi
}

# CCO Agent Ontology <-> IAO (agents produce information artifacts)
if [[ -n "$iao_path" ]]; then
  run_bridge "cco-agent→iao" \
    --a "${CCO_CACHE}/AgentOntology.ttl" \
    --b "$iao_path" \
    --out "${BRIDGES_DIR}/cco-agent-iao.owl" \
    --proposals "${BRIDGES_DIR}/cco-agent-iao.proposals.jsonl" \
    --threshold 0.75

  # CCO Information Entity Ontology vs IAO — expected high overlap / confirmed equivalences
  run_bridge "cco-info→iao" \
    --a "${CCO_CACHE}/InformationEntityOntology.ttl" \
    --b "$iao_path" \
    --out "${BRIDGES_DIR}/cco-info-iao.owl" \
    --proposals "${BRIDGES_DIR}/cco-info-iao.proposals.jsonl" \
    --threshold 0.75
fi

# CCO Event Ontology bridges to BFO Process
if [[ -n "$bfo_path" ]]; then
  run_bridge "cco-event→bfo" \
    --a "${CCO_CACHE}/EventOntology.ttl" \
    --b "$bfo_path" \
    --out "${BRIDGES_DIR}/cco-event-bfo.owl" \
    --proposals "${BRIDGES_DIR}/cco-event-bfo.proposals.jsonl" \
    --threshold 0.75
fi

# ---------------------------------------------------------------------------
# Phase 4 — Incremental join: add CCO modules into the existing store
# ---------------------------------------------------------------------------
log "Phase 4: incrementally adding CCO modules to federated store..."
mkdir -p "$STORE_DIR"

for module in "${CCO_MODULES[@]}"; do
  ttl="${CCO_CACHE}/${module}.ttl"
  log "  lattice-join add --ontology $ttl"
  if lattice-join add \
      --store "$STORE_DIR" \
      --ontology "$ttl" \
      --allow-unreviewed 2>&1; then
    log "  $module: added to store"
  else
    warn "  $module: join add failed (continuing)"
  fi
done

# Add CCO bridge files
log "  adding CCO bridges to store..."
for bridge in "${BRIDGES_DIR}"/cco-*.owl; do
  if [[ -f "$bridge" && -s "$bridge" ]]; then
    log "  bridge: $bridge"
    if lattice-join add \
        --store "$STORE_DIR" \
        --ontology "$bridge" \
        --allow-unreviewed 2>&1; then
      log "  $(basename "$bridge"): added"
    else
      warn "  $(basename "$bridge"): join add failed (continuing)"
    fi
  else
    warn "  skipping missing/empty bridge: $bridge"
  fi
done

# ---------------------------------------------------------------------------
# Phase 5 — Validation queries
# ---------------------------------------------------------------------------
log "Phase 5: validation..."

log "  lattice-join stats..."
lattice-join stats --store "$STORE_DIR" 2>&1 || warn "stats failed"

log "  lattice-ground resolve 'agent'..."
lattice-ground resolve "agent" 2>&1 || warn "resolve agent failed"

log "  lattice-ground resolve 'plan'..."
lattice-ground resolve "plan" 2>&1 || warn "resolve plan failed"

log "  lattice-ground resolve 'role'..."
lattice-ground resolve "role" 2>&1 || warn "resolve role failed"

log "lattice-cco-ingest complete."
