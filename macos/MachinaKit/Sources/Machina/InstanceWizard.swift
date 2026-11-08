import SwiftUI
import MachinaKit

/// Fleet Cloud launch wizard: image, flavor, network and key pair, name and count, user data, review.
/// Shown in place of the Instances table (not as a sheet) so the window-level confirmation can appear over it.
struct InstanceWizard: View {
    @Environment(AppModel.self) private var model
    let onClose: () -> Void

    enum Step: Int, CaseIterable, Identifiable {
        case image, flavor, network, details, userData, review
        var id: Int { rawValue }
        var title: String { ["Image", "Flavor", "Network", "Name & count", "User data", "Review"][rawValue] }
    }

    @State private var step: Step = .image
    @State private var launch = InstanceLaunch()
    @State private var catalog: InstanceCatalog?
    @State private var loadError: String?
    @State private var imageFilter = ""
    @State private var submitting = false

    private var image: InstanceImage? { catalog?.images.first { $0.name == launch.imageName } }
    private var flavor: InstanceFlavor? { catalog?.flavors.first { $0.id == launch.flavorId } }

    /// What blocks leaving the current step (nil = can continue).
    private var blocker: String? {
        switch step {
        case .image: return launch.imageName.isEmpty ? "Pick an image." : nil
        case .flavor: return launch.flavorId.isEmpty ? "Pick a flavor." : nil
        case .network: return nil
        case .details: return Self.reason { try launch.validateName() }
        case .userData: return launch.userData.utf8.count > InstanceLaunch.maxUserDataBytes ? "User data is over the \(InstanceLaunch.maxUserDataBytes) byte limit." : nil
        case .review: return Self.reason { try launch.validate() }
        }
    }

    private static func reason(_ check: () throws -> Void) -> String? {
        do { try check(); return nil } catch { return error.localizedDescription }
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            HStack {
                Label("Launch instance", systemImage: "plus.rectangle.on.rectangle").font(.largeTitle.weight(.semibold))
                Spacer()
                Button("Cancel", role: .cancel) { onClose() }
            }
            stepper
            PlatformErrorBanner(text: loadError ?? catalog?.problems.joined(separator: " · "))
            Group {
                if catalog == nil && loadError == nil { ProgressView("Loading images, flavors and networks…").frame(maxWidth: .infinity, maxHeight: .infinity) }
                else { content }
            }.frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
            Divider()
            HStack {
                if let b = blocker { Text(b).font(.callout).foregroundStyle(.secondary) }
                Spacer()
                Button("Back") { if let p = Step(rawValue: step.rawValue - 1) { step = p } }.disabled(step == .image || submitting)
                if step == .review {
                    Button("Launch…") { confirmLaunch() }.buttonStyle(.borderedProminent).keyboardShortcut(.defaultAction)
                        .disabled(blocker != nil || submitting || !model.canWrite).help(model.canWrite ? "" : "Your role cannot make changes")
                } else {
                    Button("Next") { if let n = Step(rawValue: step.rawValue + 1) { step = n } }.buttonStyle(.borderedProminent)
                        .keyboardShortcut(.defaultAction).disabled(blocker != nil || catalog == nil)
                }
            }
        }
        .padding(24)
        .task { await loadCatalog() }
    }

    private var stepper: some View {
        HStack(spacing: 6) {
            ForEach(Step.allCases) { s in
                HStack(spacing: 5) {
                    Text("\(s.rawValue + 1)").font(.caption.weight(.semibold)).frame(width: 18, height: 18)
                        .background(s.rawValue <= step.rawValue ? Color.accentColor : Color.secondary.opacity(0.3), in: .circle)
                        .foregroundStyle(s.rawValue <= step.rawValue ? .white : .primary)
                    Text(s.title).font(.callout).foregroundStyle(s == step ? .primary : .secondary)
                }
                if s != Step.allCases.last { Divider().frame(width: 14) }
            }
        }
    }

    @ViewBuilder private var content: some View {
        switch step {
        case .image:
            let images = (catalog?.images ?? []).filter { imageFilter.isEmpty || $0.name.localizedCaseInsensitiveContains(imageFilter) || ($0.description ?? "").localizedCaseInsensitiveContains(imageFilter) }
            VStack(alignment: .leading) {
                TextField("Filter images", text: $imageFilter).textFieldStyle(.roundedBorder)
                if images.isEmpty { ContentUnavailableView("No images", systemImage: "opticaldisc", description: Text("Add an image on the Images page first.")) }
                else {
                    List(images, selection: Binding(get: { launch.imageName.isEmpty ? nil : image?.id }, set: { id in launch.imageName = catalog?.images.first { $0.id == id }?.name ?? "" })) { i in
                        VStack(alignment: .leading, spacing: 2) {
                            HStack { Text(i.name).font(.headline); if let v = i.version { Text("v\(v)").foregroundStyle(.secondary) }; Spacer(); if let o = i.osFamily { Text(o).font(.caption).foregroundStyle(.secondary) } }
                            if let d = i.description, !d.isEmpty { Text(d).font(.callout).foregroundStyle(.secondary).lineLimit(2) }
                        }.tag(i.id)
                    }
                }
            }
        case .flavor:
            if (catalog?.flavors ?? []).isEmpty {
                ContentUnavailableView("No flavors", systemImage: "square.grid.3x3", description: Text("Create a flavor on the Flavors page first."))
            } else {
                List(catalog?.flavors ?? [], selection: Binding(get: { launch.flavorId.isEmpty ? nil : launch.flavorId }, set: { launch.flavorId = $0 ?? "" })) { f in
                    HStack { Text(f.name).font(.headline); Spacer(); Text(f.summary).foregroundStyle(.secondary).monospacedDigit() }.tag(f.id)
                }
            }
        case .network:
            Form {
                Picker("Network", selection: $launch.network) {
                    Text("Server default").tag("")
                    ForEach(catalog?.networks ?? []) { Text($0.name + ($0.backend.map { " (\($0))" } ?? "")).tag($0.name) }
                }
                Picker("Key pair", selection: $launch.keyName) {
                    Text("None").tag("")
                    ForEach(catalog?.keypairs ?? []) { Text($0.name).tag($0.name) }
                }
                if (catalog?.keypairs ?? []).isEmpty { Text("No saved key pairs. Import one on the Key pairs page to get SSH access without a password.").font(.callout).foregroundStyle(.secondary) }
            }.formStyle(.grouped)
        case .details:
            Form {
                TextField("Name", text: $launch.name).autocorrectionDisabled()
                Stepper("Count: \(launch.count)", value: $launch.count, in: 1...InstanceLaunch.maxCount)
                if launch.count > 1 { Text("Instances: \(launch.plannedNames.prefix(4).joined(separator: ", "))\(launch.count > 4 ? ", …" : "")").font(.callout).foregroundStyle(.secondary) }
            }.formStyle(.grouped)
        case .userData:
            VStack(alignment: .leading, spacing: 6) {
                Text("Optional cloud-init user data (a #cloud-config document or a script), passed to the guest as is.").font(.callout).foregroundStyle(.secondary)
                TextEditor(text: $launch.userData).font(.system(.callout, design: .monospaced)).autocorrectionDisabled()
                    .overlay(RoundedRectangle(cornerRadius: 6).stroke(.quaternary))
                HStack {
                    Button("Insert skeleton") { if launch.userData.isEmpty { launch.userData = "#cloud-config\npackages:\n  - htop\nruncmd:\n  - echo hello > /etc/motd\n" } }
                    Spacer()
                    let n = launch.userData.utf8.count
                    Text("\(n) of \(InstanceLaunch.maxUserDataBytes) bytes").font(.caption).monospacedDigit()
                        .foregroundStyle(n > InstanceLaunch.maxUserDataBytes ? .red : .secondary)
                }
                Text("It may contain secrets: anyone who can read the instance spec can read it.").font(.caption).foregroundStyle(.secondary)
            }
        case .review:
            Grid(alignment: .leading, horizontalSpacing: 24, verticalSpacing: 8) {
                row("Image", launch.imageName + (image?.version.map { " v\($0)" } ?? ""))
                row("Flavor", flavor.map { "\($0.name) · \($0.summary)" } ?? launch.flavorId)
                row("Network", launch.network.isEmpty ? "Server default" : launch.network)
                row("Key pair", launch.keyName.isEmpty ? "None" : launch.keyName)
                row("Name", launch.name)
                row("Count", "\(launch.count)" + (launch.count > 1 ? " (\(launch.plannedNames.prefix(3).joined(separator: ", "))\(launch.count > 3 ? ", …" : ""))" : ""))
                row("User data", launch.userData.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty ? "None" : "\(launch.userData.utf8.count) bytes")
            }
        }
    }

    private func row(_ k: String, _ v: String) -> some View {
        GridRow { Text(k).foregroundStyle(.secondary); Text(v).textSelection(.enabled) }
    }

    private func loadCatalog() async {
        do {
            let c = try await model.api().instanceCatalog()
            catalog = c
            if launch.flavorId.isEmpty, c.flavors.count == 1 { launch.flavorId = c.flavors[0].id }
        } catch { loadError = error.localizedDescription }
    }

    private func confirmLaunch() {
        guard (try? launch.validate()) != nil else { return }
        let l = launch
        let msg = "\(l.count) instance\(l.count == 1 ? "" : "s") from \(l.imageName) (\(flavor?.name ?? l.flavorId)) will be created and started."
        model.confirmAction("Launch \(l.count == 1 ? l.name : "\(l.count) instances")?", message: msg, label: "Launch", destructive: false) { [model] in
            guard model.canWrite else { model.error = "Launch: your role (\(model.role ?? "unknown")) cannot make changes"; return }
            submitting = true; defer { submitting = false }
            do {
                let r = try await model.api().runInstances(l)
                model.error = nil
                model.notice = "\(r.created) of \(r.requested) instance\(r.requested == 1 ? "" : "s") queued" + (r.error.map { " (\($0))" } ?? "")
                if r.created > 0 { onClose() }
            } catch { model.error = "Launch: \(error.localizedDescription)" }
        }
    }
}
