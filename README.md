# Musicton

Eine Techno-DAW für Linux in Rust, gebaut für Jammen, Live-Performance und Produktion: Session View zum Jammen und Live-Spielen, Arrangement View zum Fertigstellen von Tracks, eingebaute Instrumente, 36 Audio-Effekte, MIDI-Effekte, Audiospuren mit Warping und MIDI-Controller-Mapping.

Das Repo enthält außerdem den **B-303** (TB-303-artige Bassline mit Sequencer) als eigenständiges **VST3/CLAP-Plugin** für andere DAWs, mit derselben Klangerzeugung und Oberfläche wie in Musicton.

## Repo-Aufbau

```
musicton/              die DAW (Binary `musicton` + MCP-Bridge `musicton-mcp`)
b303-plugin/           B-303 als VST3/CLAP-Plugin (nih-plug)
crates/musicton-dsp/   gemeinsame DSP-Bausteine (Oszillatoren, Filter, FFT …)
crates/b303-core/      B-303-Klangerzeugung, Sequencer, Parameterliste, Presets
crates/b303-gui/       B-303-Oberfläche (egui), genutzt von DAW und Plugin
xtask/                 Bundler für die Plugin-Dateien
```

Beim ersten Start rendert Musicton seine eigene Sample-Library (Musicton Core, CC0) und lädt ein Demo-Projekt („Warehouse Demo“, 130 BPM). Du kannst also sofort Szenen starten, Clips bearbeiten und exportieren.

## Starten

```fish
fish_add_path ~/.cargo/bin      # einmalig, falls cargo nicht gefunden wird
cd ~/repos/Musicton
cargo run --release
```

Oder installieren (Programm-Menü-Eintrag und `musicton`-Befehl):

```fish
./install.sh            # Musicton
./install.sh --plugin   # zusätzlich das B-303-Plugin nach ~/.vst3 und ~/.clap
```

### B-303-Plugin bauen

```fish
cargo xtask bundle b303-plugin --release
```

Ergebnis: `target/bundled/B-303.vst3` und `target/bundled/B-303.clap`. Kopiere sie nach `~/.vst3/` bzw. `~/.clap/` und lass die DAW neu nach Plugins suchen. Das Plugin hat den internen Sequencer (8 Patterns, im Projekt der DAW gespeichert, synchron zum Host-Transport), spielt aber auch per MIDI.

Audio läuft über ALSA bzw. PipeWire/PulseAudio (Standardgerät). Ein angeschlossener MIDI-Controller wird automatisch verbunden (Menü ☰ → MIDI Input).

## Funktionen

**Ansichten**
- Session View: Clip-Raster mit Szenen, Clip-Stop-Buttons, Mixer mit Sends, Pan, Fadern mit Metern, Solo/Activator, Crossfader-Zuweisung A/B
- Arrangement View: Timeline, Loop-Klammer, Start-Marker, Clips verschieben, loopen und kürzen, zwischen Spuren ziehen, Automations-Lanes mit Breakpoints für jeden Parameter
- Detail View: Clip View (Step-Sequencer, Piano Roll mit Velocity-, Accent- und Slide-Lane, Audio-Clip-Editor) und Device View (Kette aus Macros, MIDI-Effekten, Instrument und Audio-Effekten)
- Browser mit Kategorien und Suche, Info View (Hover-Hilfe zu jedem Element)

**Instrumente:** B-303 Bassline (TB-303-Nachbau: Diodenkaskaden-artiges Filter, Accent-Sweep, Slide, interner Sequencer mit 8 Patterns, Shuffle, Scale, 5 Play-Modes, Drive- und Delay-Sektion, eigenes Plugin-Fenster im Stil der Roland-Cloud-Version) · Nexa (Nexus-artiger Rompler: 53 Presets in 10 Kategorien, bis zu 4 Layer, Macros, Arpeggiator, Trancegate, Effekte) · Drum Machine (10 Lanes, synthetisiert oder mit WAV pro Lane) · Acid Bass (303-Style mit Accent/Slide) · Poly Synth · Simpler (Classic/One-Shot/Slice, Tempo-Sync) · Operator (4-OP FM, 8 Algorithmen) · Wavetable (5 Tabellen, LFO/Env auf Position) · Tension (Karplus-Strong-Saite)

**Audio-Effekte (36):** Auto Filter, Auto Pan, Beat Repeat, Channel EQ, Chorus, Compressor, Corpus, Drum Buss, Dynamic Tube, Echo, EQ Eight (mit Kurve), EQ Three, Erosion, Filter Delay, Flanger, Frequency Shifter, Gate, Glue Compressor, Grain Delay, Limiter, Multiband Dynamics (OTT-fähig), Overdrive, Pedal, Phaser, Ping Pong Delay, Redux, Resonators, Reverb, Saturator, Sidechain, Simple Delay, Spectrum, Tuner, Utility, Vinyl Distortion, Vocoder

**MIDI-Effekte:** Arpeggiator, Chord, Note Length, Pitch, Random, Scale, Velocity

**Audio:** Audiospuren mit Warp-Modi Beats (Time-Stretch), Re-Pitch und Off, automatische Tempo-Erkennung von Loops, Audio-Aufnahme vom Eingang (Spur scharf schalten ● + Record)

**Plugin-Fenster:** B-303 und Nexa erscheinen in der Device-Kette als kompakter Plugin-Device mit den wichtigsten Parametern. Der 🔧-Button (oder Doppelklick auf den Titel) öffnet den Editor in einem eigenen Fenster, Ctrl+Alt+P blendet alle Plugin-Fenster ein/aus. Neue B-303/Nexa-Spuren öffnen ihr Fenster automatisch.

**Live-Performance:** Clip- und Szenen-Launch mit globaler Quantisierung · Follow Actions · Gruppenspuren (Ctrl+G) · Rack-Macros · Crossfader · MIDI-Learn und Tastatur-Mapping für jeden Regler, Clip und jede Szene (Rechtsklick) · Arrangement Record (Session-Performance auf die Timeline aufnehmen) · Capture MIDI · Session Record · Back to Arrangement · Tap Tempo, Swing, Metronom

**Projekt:** Speichern/Laden (`.mton`), Autosave, Undo/Redo, Export als WAV (16/24/32 Bit, Arrangement oder Szene, Master oder einzelne Spuren/Stems)

## Samples

- **Musicton Core** (CC0): 57 Kicks, Snares, Claps, Hats, Percs, Drum-, Bass- und Synth-Loops (130 BPM), Stabs und FX. Entsteht beim ersten Start in `~/.local/share/musicton/samples/`.
- Menü ☰ → Samples → *Download free CC0 drum samples* lädt das CC0-Pack [Boochi44/free-drum-samples](https://github.com/Boochi44/free-drum-samples) (benötigt `git`).
- Eigene WAVs: in `~/.local/share/musicton/samples/` oder `~/Samples` ablegen oder im Browser über „Add Folder…“ einen Ordner hinzufügen.

## Tastatur

| Taste | Funktion |
|---|---|
| Leertaste | Play/Stop |
| Tab | Session ↔ Arrangement |
| Shift+Tab | Clip View ↔ Device View |
| A W S E D F T G Y H U J K O L P | Noten spielen (auf Drum-Spuren: Lanes) |
| Z / X | Oktave runter/hoch |
| C / V | Velocity runter/hoch |
| M | Metronom |
| F9 | Record |
| Enter | Ausgewählten Clip starten |
| Entf | Clip/Note löschen |
| Ctrl+D | Clip duplizieren |
| Ctrl+G | Spur gruppieren |
| Ctrl+Z / Ctrl+Y | Undo/Redo |
| Ctrl+S / Ctrl+O / Ctrl+N | Speichern/Öffnen/Neu |
| Ctrl+E | Export |
| Ctrl+Mausrad | Arrangement zoomen |

## Noch nicht enthalten

Netzwerk-Tempo-Sync, Audio-to-MIDI, Sampler-Multisampling, Clip-Hüllkurven und Tempo-Automation, Freeze/Flatten, VST/LV2-Plug-ins.

## Fernsteuerung per MCP (für KI-Assistenten)

Musicton öffnet beim Start einen lokalen Steuer-Port (`127.0.0.1:47800`, nur lokal erreichbar; abschalten mit `MUSICTON_NO_REMOTE=1`, anderer Port mit `MUSICTON_PORT`). Der MCP-Server `musicton-mcp` übersetzt MCP-Tool-Aufrufe darauf. Claude Code findet ihn über die `.mcp.json` im Projektordner.

Die 46 Tools decken ab:
- **Zustand:** Projekt, Spuren, Clips mit Noten, Devices mit allen Parametern, Meter
- **Transport und Launch:** Abspielen, Stoppen, Clips und Szenen starten
- **Bearbeiten:** Spuren, Clips, Noten, Devices, Parameter, Automation, Arrangement
- **Messen:** Offline-Rendern mit Pegelanalyse (Peak, RMS, Clipping, Bass/Mitten/Höhen-Verteilung)
- **Oberfläche:** Screenshots und Maus-/Tastatur-Eingaben

`musicton_launch_app` startet die App, falls sie nicht läuft (mit `--x11`). Hintergrund: Unter Wayland pausiert KDE verdeckte oder minimierte Fenster, dann kann die App keine Befehle abarbeiten. Mit `musicton --x11` (XWayland) läuft sie immer weiter.

Protokoll des Steuer-Ports (für eigene Skripte): eine JSON-Zeile `{"cmd": "...", "args": {...}}` pro Anfrage, Antwort `{"ok": true, "result": ...}`.

## Entwicklung

```fish
cargo test --release -p musicton   # Engine-Tests: alle Effekte, Instrumente, MIDI-Effekte, Warping, Gruppen
cargo build --release --workspace  # alles inklusive Plugin
```

Debug-Hilfen: `MUSICTON_SHOT=datei.ppm` rendert einige Frames, speichert einen Screenshot und beendet sich (zusätzlich `MUSICTON_VIEW=arrangement`, `MUSICTON_CLIP=spur,szene`, `MUSICTON_TRACK=n`, `MUSICTON_ADD_FX=Name,Name`).

## Lizenz

GPL-3.0-or-later (siehe `LICENSE`). Die VST3-Anbindung des Plugins setzt die GPLv3 voraus. „TB-303“ und „Nexus“ sind Marken ihrer Inhaber; Musicton ist ein unabhängiges Projekt und mit ihnen nicht verbunden.
