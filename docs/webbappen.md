# Uppdrag till Lovable (skickas när agenten är ledig — ett uppdrag åt gången)

Fixra finns nu som skrivbordsapp (Mac/Windows) som laddar app.fixra.se. Skalet lägger
`window.fixraDesktop = { platform, version, origin, startPath, onNavigate }` och klassen
`fixra-desktop` på `<html>`. Gör tre små saker i webbappen, inget annat:

1. **Sidbyte utan omladdning.** I rotlayouten (klientsidan, efter att routern finns):
   `if (window.fixraDesktop) window.fixraDesktop.onNavigate = (p) => router.navigate({ to: p })`.
   Typa `window.fixraDesktop` som valfri i en global .d.ts.

2. **Inloggningslänken ska landa i appen.** I `login-page.tsx` → `sendMagicLink`: är
   `window.fixraDesktop` satt, använd `emailRedirectTo: "fixra://auth/callback"` i stället för
   `${base}/auth/callback`. Lägg `fixra://auth/callback` i Supabase Auth → Redirect URLs
   (både Lovable Cloud-projektet och wqptufxlagbvowxtgcbq). Kontrollera att `/auth/callback`
   hanterar token i fragmentet (`#access_token=…`) — skalet skickar vidare fragmentet orört.

3. **Märke på dockikonen.** I `app-shell.tsx`, där `requests.data?.new` och `extraTime.data?.pending`
   räknas: `window.__TAURI__?.core.invoke("set_badge", { count: nya förfrågningar + väntande extra tid })`
   när värdet ändras, och `count: 0` vid utloggning. Anropet får aldrig kasta — svälj fel.

Prov: i vanlig webbläsare ska inget av detta synas eller ändra beteende (`window.fixraDesktop` saknas).
