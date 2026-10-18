# Releases

Musicton und das B-303-Plugin werden **unabhängig voneinander** versioniert, beide nach [SemVer](https://semver.org/lang/de/).

| Produkt | Version steht in | Changelog | Tag |
|---|---|---|---|
| Musicton | `musicton/Cargo.toml` | `CHANGELOG.md` | `musicton-vX.Y.Z` |
| B-303 Plugin | `b303-plugin/Cargo.toml` | `b303-plugin/CHANGELOG.md` | `b303-vX.Y.Z` |

Die internen Crates (`musicton-dsp`, `b303-core`, `b303-gui`, `xtask`) behalten die Workspace-Version und werden nicht einzeln veröffentlicht.

**Wann welche Stelle erhöht wird (solange 0.x):** Minor (`0.2.0`) für neue Funktionen oder inkompatible Projektdateien, Patch (`0.1.1`) für Bugfixes. Ab `1.0.0` gilt SemVer streng: Major für Brüche bei Projektdateien oder Plugin-Parametern (Automation in Host-Projekten), Minor für Neues, Patch für Fixes. Versionen `0.x` werden auf GitHub als Pre-Release markiert.

## Ablauf

1. Änderungen laufend unter `## [Unreleased]` im passenden Changelog eintragen.
2. Release schneiden:
   ```bash
   scripts/release.sh musicton 0.2.0     # oder: scripts/release.sh b303 0.1.1
   git push && git push origin musicton-v0.2.0
   ```
3. Der Workflow **Release** baut die Installer auf Linux, macOS und Windows und hängt sie an ein GitHub-Release mit den Notizen aus dem Changelog. Ein bestehender Tag lässt sich unter *Actions → Release → Run workflow* neu bauen.

## Installer

| System | Musicton | B-303 |
|---|---|---|
| Linux | `.deb` (systemweit) und `.tar.gz` mit `install.sh` (nach `~/.local`) | `.tar.gz` mit `install.sh` (nach `~/.vst3` und `~/.clap`) |
| macOS | `.dmg` mit `Musicton.app`, Universal (Apple Silicon und Intel) | `.pkg` nach `/Library/Audio/Plug-Ins/{VST3,CLAP}` |
| Windows | Inno-Setup-Installer (x64) | Inno-Setup-Installer nach `Common Files\{VST3,CLAP}` |

Lokal bauen geht auf dem jeweiligen System so: `packaging/linux/build-musicton.sh 0.1.0`, `packaging/macos/build-b303.sh 0.1.0` oder `packaging\windows\build.ps1 -Product musicton -Version 0.1.0`. Das Ergebnis landet in `dist/`.

## Signieren (noch offen)

Die Builds sind derzeit **nicht signiert**. Unter macOS sind sie nur ad hoc signiert, Nutzer öffnen sie per Rechtsklick → „Öffnen“. Unter Windows erscheint SmartScreen. Für signierte Releases braucht es:

- **macOS:** eine Apple-Developer-ID (99 $/Jahr). Danach `codesign` mit Developer ID und `xcrun notarytool`. Zertifikat und Passwort kommen als Repo-Secrets dazu.
- **Windows:** ein Code-Signing-Zertifikat, z. B. über Azure Trusted Signing. Danach `signtool` im Workflow.
