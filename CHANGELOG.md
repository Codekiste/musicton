# Changelog – Musicton

Alle nennenswerten Änderungen an Musicton. Format nach [Keep a Changelog](https://keepachangelog.com/de/1.1.0/), Versionen nach [SemVer](https://semver.org/lang/de/).
Das B-303-Plugin hat einen eigenen Changelog: [b303-plugin/CHANGELOG.md](b303-plugin/CHANGELOG.md).

## [Unreleased]

### Hinzugefügt
- Neue Oberfläche:
  - Schiefergraue Studio-Optik mit blauem Akzent, abgerundeten Bedienelementen und Verläufen
  - Transportleiste mit LCD-Anzeige (Takt/Beat/Div/Tick, Zeit, Tempo zum Ziehen, Taktart, Tonart) und runden Transporttasten (Anfang, Stop, Play, Aufnahme, Cycle, Metronom)
  - Inspector links mit Region- und Spur-Einstellungen, zwei Kanalzügen (gewählte Spur und Master) und Kurzhilfe; Browser rechts
  - Timeline als Startansicht („Tracks“) mit neuen Spurköpfen (Nummer in Spurfarbe, runde M/S/R/A-Tasten, Lautstärke-Regler), abgerundeten Regionen und gelbem Cycle-Bereich
- Mixer-Ansicht (Button „Mixer“, Strg+M): Kanalzüge in voller Höhe mit EQ-Anzeige, Instrument, Insert-Slots (Bypass, Reihenfolge, Entfernen, Hinzufügen), Send-Reglern, Output-Routing, Pan, großem Fader mit dB-Skala, Stereo-Meter mit Peak-Hold, Peak-Anzeige, Mute/Solo/Record und Spurname in Spurfarbe; Bereiche einzeln ein- und ausblendbar; per MCP mit `set_view view=mixer`

- Rechtsklick-Menü am Spurkopf (Launcher, Timeline, Mixer):
  - Ausschneiden, Kopieren, Einfügen und Duplizieren von Spuren, „Neue Spur mit gleichen Einstellungen“
  - Info-Text pro Spur, Spursymbol, Spurfarben automatisch vergeben
  - Spuren einfügen: Instrument- und Audiospur (aus der eigenen Standardspur), Return-Spur (bis zu 6 Returns, Sends pro Return)
  - Gruppe mit gemeinsamem Bus oder Ordner nur zur Organisation, „Unbenutzte Spuren löschen“
  - Freeze/Unfreeze, Freeze und Flatten, Bounce in Place (mit Effekten oder Effekte bleiben live), Bounce in neue Spur
  - Spur ausblenden, versteckte Spuren anzeigen, Spur schützen (gegen Bearbeiten gesperrt), alle Clips auswählen
  - Konfigurierbarer Spurkopf in der Timeline (Freeze, Schutz, Ausblenden, Aufnahme, Take-Lanes)
  - Instrument ersetzen, „Als Standardspur speichern“
- Rechtsklick-Menü an Clips:
  - Ausschneiden, Kopieren, Einfügen, Duplizieren, verknüpfte Kopie (Bearbeiten wirkt auf alle Kopien), Clip stummschalten
  - Teilen an Mausposition oder Abspielposition, ausgewählte Clips verbinden (Strg-Klick für Mehrfachauswahl), auf Loop-Bereich zuschneiden, Loop bis zum nächsten Clip
  - Audio: Reverse, Warp-Modus, Tempo aus Länge erkennen, Tonhöhenkorrektur, in Sampler-Spur umwandeln (Slicer, Beat Pads, Multi Sampler), Audio zu MIDI, Stems
  - MIDI: Quantisieren, Legato, Notenlängen, Velocity (fest, ausgleichen, humanisieren)
  - Export als Audiodatei, als Loop mit Tempo im Namen oder als MIDI-Datei
  - Launcher-Clips: Next Action, gespielte Noten hier aufnehmen, Szene einfügen
- Rechtsklick auf freie Timeline-Fläche: Einfügen, Stille einfügen, Zeit löschen, Zeit duplizieren (auf dem Loop-Bereich)
- Take-Lanes fürs Comping: Beim Aufnehmen überschriebene Clips landen auf einer Take-Lane, ein Klick übernimmt den Take
- MCP: `track_action`, `clip_action` und `time_action` für alle Menüfunktionen

### Behoben
- Horizontale Scrollbalken: Die Timeline hat jetzt Scrollbalken (horizontal und vertikal), alle anderen Bereiche zeigen ihre Balken dauerhaft statt nur beim Überfahren
- Beim Kürzen eines Clips von vorne (überlappende Clips) beginnt der Rest jetzt an der richtigen Stelle im Inhalt
- Bricht die Audioausgabe ab (Soundserver-Neustart, Interface getrennt), verbindet sich Musicton automatisch neu, statt die Fehlermeldung endlos ins Log zu schreiben
- VST3-Plug-ins mit getrenntem Controller zeigen ihren Editor: Component und Controller werden über Connection Points verbunden, der Host stellt Nachrichten (IMessage/IAttributeList) bereit
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
