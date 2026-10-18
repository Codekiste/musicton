# Changelog – Musicton

Alle nennenswerten Änderungen an Musicton. Format nach [Keep a Changelog](https://keepachangelog.com/de/1.1.0/), Versionen nach [SemVer](https://semver.org/lang/de/).
Das B-303-Plugin hat einen eigenen Changelog: [b303-plugin/CHANGELOG.md](b303-plugin/CHANGELOG.md).

## [Unreleased]

### Hinzugefügt
- Mixer-Ansicht (Button „Mixer“, Strg+M): Kanalzüge in voller Höhe mit EQ-Anzeige, Instrument, Insert-Slots (Bypass, Reihenfolge, Entfernen, Hinzufügen), Send-Reglern, Output-Routing, Pan, großem Fader mit dB-Skala, Stereo-Meter mit Peak-Hold, Peak-Anzeige, Mute/Solo/Record und Spurname in Spurfarbe; Bereiche einzeln ein- und ausblendbar; per MCP mit `set_view view=mixer`

### Behoben
- Bricht die Audioausgabe ab (Soundserver-Neustart, Interface getrennt), verbindet sich Musicton automatisch neu, statt die Fehlermeldung endlos ins Log zu schreiben
- VST3-Plug-ins mit getrenntem Controller (z. B. Surge XT) zeigen ihren Editor: Component und Controller werden über Connection Points verbunden, der Host stellt Nachrichten (IMessage/IAttributeList) bereit
- Das Modulator-Panel dehnt sich nicht mehr über die ganze Device Chain aus
- Entfernte oder ersetzte Plug-ins beenden ihren Sandbox-Prozess (vorher liefen die Prozesse bis zum Programmende weiter)

## [0.1.0] - 2026-10-18

Erste öffentliche Vorabversion.

### Hinzugefügt
- Launcher (Clip-Raster mit Szenen) und Timeline (Arrangement mit Automation), Mixer mit Sends, Returns und Crossfader, Gruppenspuren
- Arranger-Spur, Section Chain, Marker, Pattern-Playlist, Clip Gain, Modulatoren, Racks mit parallelen Ketten, freies Routing und Sidechain von jeder Spur
- 15 Instrumente, darunter B-303 Bassline, Nexa, Voltage, Matrix FM, Grain Cloud, Beat Pads, Multi Sampler und Slicer
- Audio-Effekte, darunter Beat Mangler, Heat Stack, Hybrid Reverb, Transient Shaper, Maximizer, True Peak Limiter mit LUFS-Meter und Stereo Imager
- 8 MIDI-Effekte, Chord Track, Follow Chords, Chord Pads, Chord Stamps und Strum, Ghost Notes, Note Rules und Noten-Wahrscheinlichkeit
- Audio-Aufnahme, Audiospuren mit Tempo-Warping, WAV-Export als Master oder Stems, Undo/Redo, Autosave
- Eigene Sample-Library „Musicton Core“ (CC0)
- Plug-ins (VST3/CLAP) in eigener Sandbox (`musicton-plughost`), Projektversionen, Notenansicht mit MusicXML-Export, Audio zu MIDI, Stem-Splitter, Video-Spur mit MP4-Export
- Pitch Fix und Space Panner (45 Audio-Effekte)
- Fernsteuerung per MCP (70 Tools)
- Installer für Linux (.deb, .tar.gz), macOS (.dmg, Universal) und Windows (Setup)
