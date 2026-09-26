# Fixra för Mac och Windows

Skrivbordsappen för Fixra. Ett tunt, inbyggt skal (Tauri 2) runt **app.fixra.se** —
all funktionalitet kommer från webbappen, så varje ny funktion som publiceras i
Lovable finns i skrivbordsappen samma sekund. Ingen dubbel kodbas.

## Vad skalet ger utöver webbläsaren

| Funktion | Hur |
|---|---|
| Egen app i Dock/Start-menyn, eget fönster | Fönsterstorlek och placering sparas mellan starter |
| Menyrad på svenska med kortkommandon | ⌘1 Idag · ⌘2 Schema · ⌘3 Kunder · ⌘4 Offerter · ⌘5 Attest · ⌘6 Ekonomi · ⌘7 Mina pass · ⌘, Inställningar · ⌘R ladda om · ⌘[ / ⌘] bakåt/framåt · ⌘+ / ⌘− zoom (Ctrl på Windows) |
| Ikon i menyfältet (Mac) / aktivitetsfältet (Windows) | Öppna Fixra, Idag, Schema, Mina pass, sök uppdateringar |
| `fixra://`-länkar | `fixra://kunder/123` öppnar kunden i appen. Kommer länken medan appen redan är öppen landar den i samma fönster |
| Externa länkar | Google Maps, fixra.se, mejladresser m.m. öppnas i datorns vanliga program — användaren fastnar aldrig på en främmande sida |
| Hämtningar | Export, PDF, löneunderlag hamnar i Hämtade filer (aldrig överskrivning), notis + filen visas i Finder/Utforskaren |
| Offline-läge | Egen startsida i Fixras färger; utan nät visas "Ingen anslutning" och appen försöker igen själv |
| Automatiska uppdateringar | Söker vid start och var 6:e timme; frågar innan den installerar |
| Mac: röda knappen | Göm i stället för avsluta (som Mail/Slack) |

## Webbappen kan känna igen skalet

Skalet lägger in `window.fixraDesktop = { platform, version, origin, startPath, onNavigate }`
och klasserna `fixra-desktop` + `fixra-desktop-mac|windows` på `<html>`.

- **`onNavigate`**: sätt `window.fixraDesktop.onNavigate = (path) => router.navigate({ to: path })`
  så byter menyvalen sida utan omladdning. Utan den laddas sidan om (fungerar, men långsammare).
- **Notiser**: `window.__TAURI__.notification.sendNotification({ title, body })`
- **Märke på dockikonen**: `window.__TAURI__.core.invoke('set_badge', { count: 3 })` (0 tar bort)

Webbappen får inget annat — inget filsystem, inga skalkommandon (`src-tauri/capabilities/fixra-web.json`).

## Bygga och släppa

Byggen sker i GitHub Actions (`.github/workflows/release.yml`):

1. Höj `version` i `src-tauri/tauri.conf.json` och `src-tauri/Cargo.toml`.
2. `git tag v1.0.1 && git push --tags`
3. Mac (.dmg, universal — Apple Silicon + Intel) och Windows (-setup.exe + .msi) byggs,
   en release skapas och installerade appar uppdaterar sig.

### Hemligheter i GitHub (Settings → Secrets and variables → Actions)

| Namn | Krävs | Vad |
|---|---|---|
| `TAURI_SIGNING_PRIVATE_KEY` | Ja | Uppdateringsnyckeln (privat). Den publika ligger i `tauri.conf.json`. **Tappas den kan befintliga installationer aldrig uppdateras** |
| `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` | Ja (tom) | Tom sträng |
| `APPLE_CERTIFICATE`, `APPLE_CERTIFICATE_PASSWORD` | För kunder | "Developer ID Application: Telink PBX AB" (base64 .p12, giltigt till 2031-09-17) |
| `APPLE_API_PRIVATE_KEY` | För kunder | Innehållet i AuthKey_T8K6J43R53.p8 (App Store Connect-nyckeln "fixra-notarisering", Developer-behörighet) |

Utan Apple-signering startar Mac-appen bara via högerklick → Öppna. Windows utan
kodsigneringscertifikat visar SmartScreen-varning ("Mer info → Kör ändå") tills
signering läggs till (Azure Trusted Signing rekommenderas).

## Utveckla lokalt

```bash
npm install
npm run dev                                   # mot app.fixra.se
FIXRA_APP_ORIGIN=http://localhost:8080 npm run dev   # mot lokal webbapp (bara utvecklingsbygge)
cargo test --manifest-path src-tauri/Cargo.toml
```
