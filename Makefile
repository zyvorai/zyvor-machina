PREFIX ?= /usr/local
BINDIR ?= $(PREFIX)/bin
DATADIR ?= $(PREFIX)/share
SYSCONFDIR ?= /etc
UNITDIR ?= /usr/lib/systemd/system

CARGO ?= cargo
CARGO_FLAGS ?=

.PHONY: all build release debug clean install uninstall fmt fmt-check lint test web-test web-e2e regression-api regression-ops regression-lifecycle regression-platform regression-infra regression-fleet regression-fleetcloud regression-mission regression-catalog regression-fluxvm regression-hardware regression-host regression-storage regression-zeus regression-audit regression-volume regression-disk regression-power regression-net regression-guest regression-parity regression-admin regression-ai regression-operations regression-resize regression-planner regression-alerts regression-enterprise regression-apps regression-policy regression-diag regression-obs regression-security regression-provision regression-providers regression-hub regression-hunt regression-atlas regression-guestkit regression-batch regression-aiops regression-watchdog regression-linuxhost regression-firewallx regression-authz regression-fleetx regression-ui regression-ui-settings regression-ui-wizards regression-ui-security regression-ui-k8s-os regression-ui-mission regression-ui-catalog regression-ui-hardware regression-ui-host regression-ui-storage regression-ui-zeus regression-ui-audit regression-ui-volume regression-ui-power regression-ui-net regression-ui-guest regression-ui-parity regression-ui-admin regression-ui-ai regression-ui-operations regression-ui-resize regression-ui-planner regression-ui-alerts regression-ui-enterprise regression-ui-apps regression-ui-policy regression-ui-diag regression-ui-obs regression-ui-fabric regression-ui-provision regression-ui-providers regression-ui-hub regression-ui-hunt regression-ui-atlas regression-ui-guestkit regression-ui-batch regression-ui-aiops regression-ui-watchdog regression-ui-linuxhost regression-ui-firewallx regression-ui-authz regression-ui-fleetx regression-pages regression-setup check web web-clean start stop restart status deploy run-daemon bpf bpf-deps bpf-test bpf-cni-test help

all: release web ## Build everything (Rust + web)

build: ## Build in debug mode
	$(CARGO) build --workspace $(CARGO_FLAGS)

release: ## Build in release mode
	$(CARGO) build --workspace --release $(CARGO_FLAGS)

release-pg: ## Build the controller's PostgreSQL build to target/release/machina-controller-pg (Linux host)
	$(CARGO) build --release -p machina-controller --no-default-features --features postgres --target-dir target/pg $(CARGO_FLAGS)
	install -Dm755 target/pg/release/machina-controller target/release/machina-controller-pg

debug: build ## Alias for build

clean: web-clean ## Remove all build artifacts
	$(CARGO) clean

fmt: ## Format code
	$(CARGO) fmt --all

fmt-check: ## Check code formatting
	$(CARGO) fmt --all -- --check

lint: ## Run clippy lints
	$(CARGO) clippy --workspace -- -D warnings

test: ## Run Rust + web unit tests
	$(CARGO) test --workspace
	cd web && npm test

web-test: ## Run web unit tests (vitest)
	cd web && npm test

web-e2e: web ## Build web and run Playwright smoke tests
	cd web && npm run test:e2e

regression-setup: ## Install deps for scripts/regression (CDP + API sweeps)
	cd scripts/regression && npm install

LOOPS ?= 1

regression-api: ## Live API heartbeat sweep (MACHINA_BASE_URL / USER / PASS; LOOPS=N)
	cd scripts/regression && npm install --silent && node api-sweep.js --loops $(LOOPS)

regression-ops: ## Live interactive ops (power, screenshot, volumes, clone guard)
	cd scripts/regression && npm install --silent && node ops-interactive.js

regression-lifecycle: ## Live disk/nic/rename/clone lifecycle
	cd scripts/regression && npm install --silent && node ops-lifecycle.js

regression-platform: ## Live platform console/precheck/sync/pause + KubeVirt guard
	cd scripts/regression && npm install --silent && node ops-platform.js

regression-infra: ## Live networks/node/metrics/platform inventory/AI/Zeus firewall reads
	cd scripts/regression && npm install --silent && node ops-infra.js

regression-fleet: ## Live devices/services/catalog/batch power/Fleet Cloud+K8s status
	cd scripts/regression && npm install --silent && node ops-fleet.js

regression-fleetcloud: ## Native Fleet Cloud CRUD (flavors/keypairs/SGs/LBs/port-forwards)
	cd scripts/regression && npm install --silent && node ops-fleetcloud.js

regression-mission: ## Live browse disks/FS/fleet activity/reports/observability/Atlas
	cd scripts/regression && npm install --silent && node ops-mission.js

regression-catalog: ## Live jobs/audit/guest-health/network CRUD/HA/CD-ROM guards
	cd scripts/regression && npm install --silent && node ops-catalog.js

regression-fluxvm: ## Live FluxVM: consoles, snapshots, backup, hotplug, NIC, migration, controller migrate + HA re-create
	cd scripts/regression && npm install --silent && node ops-fluxvm.js

regression-hardware: ## Live hardware inventory/compat/SOC/K8s/send-key
	cd scripts/regression && npm install --silent && node ops-hardware.js

regression-host: ## Live host stats/PCI/USB/secrets CRUD/rightsizing/Zeus firewall
	cd scripts/regression && npm install --silent && node ops-host.js

regression-storage: ## Live health/session + storage/network live inventory + discover
	cd scripts/regression && npm install --silent && node ops-storage.js

regression-zeus: ## Live Zeus firewall deep + API keys/webhooks/nwfilter/cordon
	cd scripts/regression && npm install --silent && node ops-zeus.js

regression-audit: ## Live audit/templates/compliance/simulate/terminal session
	cd scripts/regression && npm install --silent && node ops-audit.js

regression-volume: ## Live volume CRUD + VM console/observability
	cd scripts/regression && npm install --silent && node ops-volume.js

regression-disk: ## Live volume resize/clone + disk attach/detach
	cd scripts/regression && npm install --silent && node ops-disk.js

regression-power: ## Live classic+platform pause/resume tasks + NIC inventory
	cd scripts/regression && npm install --silent && node ops-power.js

regression-net: ## Live classic NIC attach/detach ↔ platform nics + HA/SOC/events
	cd scripts/regression && npm install --silent && node ops-net.js

regression-guest: ## Live VM doctor/guest health/pending-config + AI cost/capacity
	cd scripts/regression && npm install --silent && node ops-guest.js

regression-parity: ## Live batch parity + policy/marketplace/upgrade/cloud-init
	cd scripts/regression && npm install --silent && node ops-parity.js

regression-admin: ## Live enrollment tokens + platform NIC/autostart tasks + diagnose/NMI
	cd scripts/regression && npm install --silent && node ops-admin.js

regression-ai: ## Live Zeus AI twin/graph/incidents/compliance/cost hubs
	cd scripts/regression && npm install --silent && node ops-ai.js

regression-operations: ## Live developer/support/runbooks/fleet desktop + guest agent negatives
	cd scripts/regression && npm install --silent && node ops-ops.js

regression-resize: ## Live platform vCPU/memory resize + spice-to-vnc + AI troubleshoot
	cd scripts/regression && npm install --silent && node ops-resize.js

regression-planner: ## Live VM schedules / jobs / blueprints + AI planner writes
	cd scripts/regression && npm install --silent && node ops-planner.js

regression-alerts: ## Live alert-rules / SOC playbooks / backup targets+schedules
	cd scripts/regression && npm install --silent && node ops-alerts.js

regression-enterprise: ## Live maintenance/cordon/enterprise/network segment smoke
	cd scripts/regression && npm install --silent && node ops-enterprise.js

regression-apps: ## Live application groups / fleet desktop / SOC ops / AI firewall
	cd scripts/regression && npm install --silent && node ops-apps.js

regression-policy: ## Live templates/policy quotas/marketplace/vault/cost smoke
	cd scripts/regression && npm install --silent && node ops-policy.js

regression-diag: ## Live diagnose/health/developer/air-gap settings smoke
	cd scripts/regression && npm install --silent && node ops-diag.js

regression-obs: ## Live observability/compliance/consolehub/reports smoke
	cd scripts/regression && npm install --silent && node ops-obs.js

regression-security: ## Live Zeus security fabric/multisite/enforcement smoke
	cd scripts/regression && npm install --silent && node ops-security.js

regression-provision: ## Live provision/join/IaC-export/guestkit schema smoke
	cd scripts/regression && npm install --silent && node ops-provision.js

regression-providers: ## Live AI providers/prompts/content/blueprints smoke
	cd scripts/regression && npm install --silent && node ops-providers.js

regression-hub: ## Live network/webhook/users/HA/operations hub smoke
	cd scripts/regression && npm install --silent && node ops-hub.js

regression-hunt: ## Live AI hunt/security + firewall plan + snapshot smoke
	cd scripts/regression && npm install --silent && node ops-hunt.js

regression-atlas: ## Live Atlas storage status/disabled + schema negatives
	cd scripts/regression && npm install --silent && node ops-atlas.js

regression-guestkit: ## Live guestkit VM doctor/migrate-plan smoke
	cd scripts/regression && npm install --silent && node ops-guestkit.js

regression-batch: ## Live VM batch power/snapshots/delete schema negatives
	cd scripts/regression && npm install --silent && node ops-batch.js

regression-aiops: ## Live AI jarvis/mission/rebalance (no execute) smoke
	cd scripts/regression && npm install --silent && node ops-aiops.js

regression-watchdog: ## Live VM watchdog + disk-export negative smoke
	cd scripts/regression && npm install --silent && node ops-watchdog.js

regression-linuxhost: ## Live host linux updates/diag/upgrade negatives
	cd scripts/regression && npm install --silent && node ops-linuxhost.js

regression-firewallx: ## Live deep Zeus firewall reads + dry-run
	cd scripts/regression && npm install --silent && node ops-firewallx.js

regression-authz: ## Live OIDC/MFA/api-keys/enroll edges
	cd scripts/regression && npm install --silent && node ops-authz.js

regression-fleetx: ## Live fleet hubs + storage/network discover smoke
	cd scripts/regression && npm install --silent && node ops-fleetx.js

regression-vmx: ## Live VM metrics/timeline/topology/console + migrate precheck
	cd scripts/regression && npm install --silent && node ops-vmx.js

regression-aifleet: ## Live Zeus AI summary/security/agents/copilot smoke
	cd scripts/regression && npm install --silent && node ops-aifleet.js

regression-healthx: ## Live VM/host doctor diagnose health-check + guest expected fails
	cd scripts/regression && npm install --silent && node ops-healthx.js

regression-devhub: ## Live developer/operations/templates/policy/cluster smoke
	cd scripts/regression && npm install --silent && node ops-devhub.js

regression-eventx: ## Live events/tasks/audit filters + notification deliver + gated negatives
	cd scripts/regression && npm install --silent && node ops-eventx.js

regression-graphx: ## Live AI graph/diagnose + Zeus approval/baremetal + storage tier negatives
	cd scripts/regression && npm install --silent && node ops-graphx.js

regression-complx: ## Live firewall compliance + enforcement soft + rename/clone/snapshot schema gates
	cd scripts/regression && npm install --silent && node ops-complx.js

regression-ui: ## Live CDP UI (Pause/Resume, platform tabs; needs Chrome :9222)
	cd scripts/regression && npm install --silent && node ui-interactive.js

regression-ui-settings: ## Live CDP settings/nav smoke (needs Chrome :9222)
	cd scripts/regression && npm install --silent && node ui-settings.js

regression-ui-wizards: ## Live CDP create/wizard/Fleet Cloud shells (needs Chrome :9222)
	cd scripts/regression && npm install --silent && node ui-wizards.js

regression-ui-security: ## Live CDP Zeus security/SOC/policy shells (needs Chrome :9222)
	cd scripts/regression && npm install --silent && node ui-security.js

regression-ui-k8s-os: ## Live CDP K8s + Fleet Cloud management shells (needs Chrome :9222)
	cd scripts/regression && npm install --silent && node ui-k8s-os.js

regression-ui-mission: ## Live CDP mission/observability/reports/GPU shells (needs Chrome :9222)
	cd scripts/regression && npm install --silent && node ui-mission.js

regression-ui-catalog: ## Live CDP jobs/audit/snapshots/HA/notifications shells (needs Chrome :9222)
	cd scripts/regression && npm install --silent && node ui-catalog.js

regression-ui-hardware: ## Live CDP VM detail/SOC/AI/K8s hardware shells (needs Chrome :9222)
	cd scripts/regression && npm install --silent && node ui-hardware.js

regression-ui-host: ## Live CDP host/placement/launchpad/rightsizing shells (needs Chrome :9222)
	cd scripts/regression && npm install --silent && node ui-host.js

regression-ui-storage: ## Live CDP storage/networks/backups shells (needs Chrome :9222)
	cd scripts/regression && npm install --silent && node ui-storage.js

regression-ui-zeus: ## Live CDP Zeus deep/API keys/connectivity shells (needs Chrome :9222)
	cd scripts/regression && npm install --silent && node ui-zeus.js

regression-ui-audit: ## Live CDP audit/templates/users/create shells (needs Chrome :9222)
	cd scripts/regression && npm install --silent && node ui-audit.js

regression-ui-volume: ## Live CDP storage volumes/observability/VM console shells (needs Chrome :9222)
	cd scripts/regression && npm install --silent && node ui-volume.js

regression-ui-power: ## Live CDP VM power/console/task shells (needs Chrome :9222)
	cd scripts/regression && npm install --silent && node ui-power.js

regression-ui-net: ## Live CDP networks/HA/SOC/events/GPU shells (needs Chrome :9222)
	cd scripts/regression && npm install --silent && node ui-net.js

regression-ui-guest: ## Live CDP observability/reports/rightsizing/doctor shells (needs Chrome :9222)
	cd scripts/regression && npm install --silent && node ui-guest.js

regression-ui-parity: ## Live CDP content/policy/marketplace/upgrade shells (needs Chrome :9222)
	cd scripts/regression && npm install --silent && node ui-parity.js

regression-ui-admin: ## Live CDP enroll/hosts/users/admin shells (needs Chrome :9222)
	cd scripts/regression && npm install --silent && node ui-admin.js

regression-ui-ai: ## Live CDP Zeus AI / approvals / incidents shells (needs Chrome :9222)
	cd scripts/regression && npm install --silent && node ui-ai.js

regression-ui-operations: ## Live CDP developer/support/operations shells (needs Chrome :9222)
	cd scripts/regression && npm install --silent && node ui-ops.js

regression-ui-resize: ## Live CDP create/vm-builder/templates resize shells (needs Chrome :9222)
	cd scripts/regression && npm install --silent && node ui-resize.js

regression-ui-planner: ## Live CDP blueprints/vm-builder/observability/SOC shells (needs Chrome :9222)
	cd scripts/regression && npm install --silent && node ui-planner.js

regression-ui-alerts: ## Live CDP notifications/backups/webhooks/SOC shells (needs Chrome :9222)
	cd scripts/regression && npm install --silent && node ui-alerts.js

regression-ui-enterprise: ## Live CDP enterprise/maintenance/network shells (needs Chrome :9222)
	cd scripts/regression && npm install --silent && node ui-enterprise.js

regression-ui-apps: ## Live CDP applications/operations/baremetal/zeus shells (needs Chrome :9222)
	cd scripts/regression && npm install --silent && node ui-apps.js

regression-ui-policy: ## Live CDP templates/marketplace/policy/cost shells (needs Chrome :9222)
	cd scripts/regression && npm install --silent && node ui-policy.js

regression-ui-diag: ## Live CDP developer/support/users/diagnose shells (needs Chrome :9222)
	cd scripts/regression && npm install --silent && node ui-diag.js

regression-ui-obs: ## Live CDP observability/reports/GPU/compliance shells (needs Chrome :9222)
	cd scripts/regression && npm install --silent && node ui-obs.js

regression-ui-fabric: ## Live CDP Zeus fabric/machine/k8s/cloud shells (needs Chrome :9222)
	cd scripts/regression && npm install --silent && node ui-fabric.js

regression-ui-provision: ## Live CDP migration/templates/launchpad shells (needs Chrome :9222)
	cd scripts/regression && npm install --silent && node ui-provision.js

regression-ui-providers: ## Live CDP AI providers/blueprints/content shells (needs Chrome :9222)
	cd scripts/regression && npm install --silent && node ui-providers.js

regression-ui-hub: ## Live CDP hubs/HA/users/webhooks shells (needs Chrome :9222)
	cd scripts/regression && npm install --silent && node ui-hub.js

regression-ui-hunt: ## Live CDP hunt/firewall/HA shells (needs Chrome :9222)
	cd scripts/regression && npm install --silent && node ui-hunt.js

regression-ui-atlas: ## Live CDP Atlas storage shells (needs Chrome :9222)
	cd scripts/regression && npm install --silent && node ui-atlas.js

regression-ui-guestkit: ## Live CDP guestkit/migration shells (needs Chrome :9222)
	cd scripts/regression && npm install --silent && node ui-guestkit.js

regression-ui-batch: ## Live CDP VMs/tasks/activity shells (needs Chrome :9222)
	cd scripts/regression && npm install --silent && node ui-batch.js

regression-ui-aiops: ## Live CDP Zeus AI ops shells (needs Chrome :9222)
	cd scripts/regression && npm install --silent && node ui-aiops.js

regression-ui-watchdog: ## Live CDP VM/backups shells (needs Chrome :9222)
	cd scripts/regression && npm install --silent && node ui-watchdog.js

regression-ui-linuxhost: ## Live CDP host/upgrade shells (needs Chrome :9222)
	cd scripts/regression && npm install --silent && node ui-linuxhost.js

regression-ui-firewallx: ## Live CDP firewall deep shells (needs Chrome :9222)
	cd scripts/regression && npm install --silent && node ui-firewallx.js

regression-ui-authz: ## Live CDP enterprise/auth shells (needs Chrome :9222)
	cd scripts/regression && npm install --silent && node ui-authz.js

regression-ui-fleetx: ## Live CDP fleet hubs/finder shells (needs Chrome :9222)
	cd scripts/regression && npm install --silent && node ui-fleetx.js

regression-ui-vmx: ## Live CDP VM detail/migration/topology shells (needs Chrome :9222)
	cd scripts/regression && npm install --silent && node ui-vmx.js

regression-ui-aifleet: ## Live CDP Zeus AI fleet shells (needs Chrome :9222)
	cd scripts/regression && npm install --silent && node ui-aifleet.js

regression-ui-healthx: ## Live CDP health/doctor/support shells (needs Chrome :9222)
	cd scripts/regression && npm install --silent && node ui-healthx.js

regression-ui-devhub: ## Live CDP developer/ops/templates/policy shells (needs Chrome :9222)
	cd scripts/regression && npm install --silent && node ui-devhub.js

regression-ui-eventx: ## Live CDP events/tasks/notifications/webhooks/SOC shells (needs Chrome :9222)
	cd scripts/regression && npm install --silent && node ui-eventx.js

regression-ui-graphx: ## Live CDP Zeus graph/approvals/hunt/enforcement shells (needs Chrome :9222)
	cd scripts/regression && npm install --silent && node ui-graphx.js

regression-ui-complx: ## Live CDP compliance/enforcement/HA/placement shells (needs Chrome :9222)
	cd scripts/regression && npm install --silent && node ui-complx.js

regression-pages: ## Live CDP page sweep (needs Chrome :9222; LOOPS=N; see scripts/regression/README.md)
	cd scripts/regression && npm install --silent && node page-sweep.js --loops $(LOOPS)

BPF_LINKER_VERSION ?= v0.11.1

bpf-deps: ## Install the eBPF toolchain (nightly rust-src + prebuilt musl bpf-linker, downloaded with retries; builds it from source if the download still fails)
	rustup toolchain install nightly --component rust-src
	@if ! command -v bpf-linker >/dev/null 2>&1; then \
	  arch=$$(uname -m); [ "$$arch" = arm64 ] && arch=aarch64; \
	  tmp=$$(mktemp -d); \
	  if curl -fsSL --retry 5 --retry-all-errors --retry-delay 10 --connect-timeout 20 "https://github.com/aya-rs/bpf-linker/releases/download/$(BPF_LINKER_VERSION)/bpf-linker-$$arch-unknown-linux-musl.tar.zst" -o "$$tmp/bpf-linker.tar.zst" \
	     && tar --zstd -xf "$$tmp/bpf-linker.tar.zst" -C "$$tmp" \
	     && bin=$$(find "$$tmp" -type f -name bpf-linker | head -1) && [ -n "$$bin" ]; then \
	    install -Dm755 "$$bin" "$${CARGO_HOME:-$$HOME/.cargo}/bin/bpf-linker"; echo "bpf-linker $(BPF_LINKER_VERSION) installed (prebuilt)"; \
	  else \
	    echo "prebuilt bpf-linker unavailable, building from source (needs llvm-config)"; $(CARGO) install bpf-linker; \
	  fi; rm -rf "$$tmp"; \
	fi

bpf: ## Build machina-bpfd with the embedded eBPF datapath (Linux; see bpf-deps)
	$(CARGO) build --release -p machina-bpf --bin machina-bpfd $(CARGO_FLAGS)
	./target/release/machina-bpfd --probe

bpf-test: ## netns enforcement/telemetry smoke test against machina-bpfd (needs root)
	sudo ./scripts/bpf/netns-smoke.sh ./target/release/machina-bpfd

bpf-cni-test: ## machina-cni plugin + datapath smoke (two netns pods, private bpfd; needs root)
	$(CARGO) build --release -p machina-bpf -p machina-cni $(CARGO_FLAGS)
	sudo ./scripts/bpf/cni-smoke.sh ./target/release

check: ## Run cargo check
	$(CARGO) check --workspace

web: ## Build web frontend
	cd web && npm install && npm run build

web-clean: ## Remove web build artifacts
	rm -rf web/dist web/node_modules

install: ## Install binaries, web UI, config, systemd unit, and mkosi workspace defs
	@test -f target/release/machina-daemon || { echo "Run 'make' or 'make release' first"; exit 1; }
	install -Dm755 target/release/machina-daemon $(DESTDIR)$(BINDIR)/machina-daemon
	@if [ -f target/release/machina-bpfd ]; then \
		install -Dm755 target/release/machina-bpfd $(DESTDIR)$(BINDIR)/machina-bpfd; \
		install -Dm644 contrib/machina-bpfd.service $(DESTDIR)$(UNITDIR)/machina-bpfd.service; \
	fi
	@if [ -f target/release/machina-cni ]; then \
		install -Dm755 target/release/machina-cni $(DESTDIR)$(BINDIR)/machina-cni; \
		install -Dm644 contrib/machina-cni.service $(DESTDIR)$(UNITDIR)/machina-cni.service; \
	fi
	@if [ "$(INSTALL_PLATFORM)" = "1" ]; then \
		install -Dm755 target/release/machina-controller $(DESTDIR)$(BINDIR)/machina-controller; \
		install -Dm755 target/release/machina-agent $(DESTDIR)$(BINDIR)/machina-agent; \
		if [ -f target/release/machina-controller-pg ]; then install -Dm755 target/release/machina-controller-pg $(DESTDIR)$(BINDIR)/machina-controller-pg; fi; \
		install -Dm755 scripts/db/machina-db.sh $(DESTDIR)$(BINDIR)/machina-db; \
		if [ -f target/release/machina-dbtool ]; then install -Dm755 target/release/machina-dbtool $(DESTDIR)$(BINDIR)/machina-dbtool; fi; \
		install -Dm644 contrib/machina-controller.service $(DESTDIR)$(UNITDIR)/machina-controller.service; \
		install -Dm644 contrib/machina-agent.service $(DESTDIR)$(UNITDIR)/machina-agent.service; \
		install -Dm644 contrib/machina-platform.env $(DESTDIR)/etc/default/machina-platform; \
	fi
	install -Dm644 contrib/machina.toml $(DESTDIR)$(SYSCONFDIR)/machina/config.toml
	install -Dm644 contrib/machina-daemon.service $(DESTDIR)$(UNITDIR)/machina-daemon.service
	@test -f $(DESTDIR)/etc/default/machina-daemon || install -Dm644 contrib/machina-daemon.default $(DESTDIR)/etc/default/machina-daemon
	@if [ -d web/dist ]; then \
		mkdir -p $(DESTDIR)$(DATADIR)/machina/web; \
		cp -r web/dist/* $(DESTDIR)$(DATADIR)/machina/web/; \
		echo "Installed web UI to $(DESTDIR)$(DATADIR)/machina/web"; \
	fi
	@if [ -d contrib/mkosi-defs ]; then \
		for ws in contrib/mkosi-defs/*/; do \
			[ -f "$${ws}mkosi.conf" ] || continue; \
			name=$$(basename "$$ws"); \
			dst=$(DESTDIR)/var/lib/machina/mkosi-defs/$$name; \
			if [ ! -d "$$dst" ]; then \
				mkdir -p "$$dst"; \
				cp -r "$${ws}." "$$dst/"; \
				echo "Installed mkosi workspace: $$name"; \
			else \
				echo "mkosi workspace already exists, skipping: $$name"; \
			fi; \
		done; \
	fi
	systemctl daemon-reload 2>/dev/null || true

uninstall: stop ## Remove installed files and stop service
	systemctl disable machina-daemon 2>/dev/null || true
	systemctl disable --now machina-cni 2>/dev/null || true
	systemctl disable --now machina-bpfd 2>/dev/null || true
	rm -f $(DESTDIR)$(BINDIR)/machina-daemon
	rm -f $(DESTDIR)$(BINDIR)/machina-bpfd
	rm -f $(DESTDIR)$(UNITDIR)/machina-bpfd.service
	rm -f $(DESTDIR)$(BINDIR)/machina-cni
	rm -f $(DESTDIR)$(UNITDIR)/machina-cni.service
	rm -f $(DESTDIR)$(BINDIR)/machina
	rm -f $(DESTDIR)$(UNITDIR)/machina-daemon.service
	rm -rf $(DESTDIR)$(DATADIR)/machina
	rm -rf $(DESTDIR)$(SYSCONFDIR)/machina
	systemctl daemon-reload 2>/dev/null || true

start: ## Start the daemon service (and machina-bpfd when installed)
	@if [ -f $(UNITDIR)/machina-bpfd.service ]; then systemctl enable --now machina-bpfd; fi
	systemctl enable --now machina-daemon

stop: ## Stop the daemon service
	systemctl stop machina-daemon 2>/dev/null || true

restart: ## Restart the daemon service
	systemctl restart machina-daemon

status: ## Show daemon service status
	@systemctl status machina-daemon 2>/dev/null || echo "Service not running"

deploy: install start ## Install and start (run 'make' first to build)
	@echo ""
	@echo "✅ Machina deployed and running"
	@echo "   🌐 Web UI:  https://localhost:5092"
	@echo "   🔗 API:     https://localhost:5092/api/v1/health"

run-daemon: build ## Run the daemon (debug)
	$(CARGO) run -p machina-daemon

help: ## Show this help
	@grep -E '^[a-zA-Z_-]+:.*?## .*$$' $(MAKEFILE_LIST) | \
		awk 'BEGIN {FS = ":.*?## "}; {printf "  📋 %-13s %s\n", $$1, $$2}'
