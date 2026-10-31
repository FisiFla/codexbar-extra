---
summary: "DeepSeek provider data sources: API key, balance, and optional detailed usage endpoints."
read_when:
  - Adding or tweaking DeepSeek balance parsing
  - Adding or tweaking DeepSeek detailed usage parsing
  - Updating API key handling
  - Documenting new provider behavior
---

# DeepSeek provider

CodexBar can use either a DeepSeek API key or a signed-in DeepSeek Platform session for the remaining credit balance.
Detailed cost and token usage comes from the Platform session; an API key cannot authenticate the private dashboard
endpoints.

## Data sources

1. **Optional API key** supplied via `DEEPSEEK_API_KEY` / `DEEPSEEK_KEY`, or selected from DeepSeek token accounts in `~/.codexbar/config.json`.
   Auto uses the API balance path when a key is configured, including on Linux. Explicit Web mode
   still requires macOS browser support; an API key alone does not provide detailed website usage.
2. **API-key balance endpoint**
   - `GET https://api.deepseek.com/user/balance`
   - Request headers: `Authorization: Bearer <api key>`, `Accept: application/json`
   - Response contains `is_available`, and a `balance_infos` array with per-currency entries
     (`total_balance`, `granted_balance`, `topped_up_balance`).
3. **Platform-session balance endpoint**
   - `GET https://platform.deepseek.com/api/v0/users/get_user_summary`
   - Request headers: `Authorization: Bearer <platform userToken>`, `Accept: application/json`
   - Used as the balance source when no API key is configured.
4. **Optional detailed usage endpoints**
   - Preferred: `GET https://platform.deepseek.com/api/v0/usage/by_api_key/amount?start=<unix>&end=<unix>&tz=<offset>`
     and `.../by_api_key/cost?...` — the same per-key daily buckets the Platform usage page loads.
   - Fallback: `GET https://platform.deepseek.com/api/v0/usage/amount?month=<month>&year=<year>`
     and `.../usage/cost?...` if the per-key endpoints fail.
   - Daily buckets use Gregorian dates at the current fixed UTC offset in seconds, matching the API across daylight-saving transitions. Monthly fallback keeps
     Gregorian UTC month selection and date parsing, and is labeled **This month**, not **Last 30 days**.
   - Cancellation stops enrichment without starting monthly fallback requests.
   - Request headers: `Authorization: Bearer <platform userToken>`, `Accept: application/json`, `x-client-platform: web`
   - These are private dashboard endpoints rather than documented public API endpoints and may change without notice.

## Platform session

CodexBar resolves the Platform `userToken` in this order:

1. An explicitly supplied `DEEPSEEK_PLATFORM_TOKEN` / `DEEPSEEK_USER_TOKEN` or a legacy
   `providers[].cookieHeader` value preserved from an existing config.
2. A prompt-free read of `userToken` from the `https://platform.deepseek.com` local-storage origin in a
   supported Chromium-family browser, using the shared browser catalog order. Local storage is plain LevelDB,
   so no Safe Storage Keychain access is involved.

The legacy config value remains a compatibility fallback so upgrading cannot silently erase a working browser-only
session. An unscoped legacy or environment token is never combined with an API-key balance; API enrichment requires
a session saved for that credential scope. New automatic imports are never written back to config.

CodexBar checks every profile of those browsers containing a parseable `userToken` against DeepSeek. Rejected or
expired sessions are omitted. Settings shows a **Browser profile** picker containing only valid sessions. Profile
identifiers are browser-scoped (`chrome:Default` from Chrome and `helium:Default` from Helium stay distinct), so a
saved selection keeps resolving after the browser list grows. With no API key, one valid session is selected
automatically; with multiple valid sessions, CodexBar reuses the saved choice or asks once.
When an API key supplies the balance, a new or changed API credential requires an explicit session selection before
website usage is combined with it. CodexBar persists a stable browser/profile identifier, not an absolute home-directory path, and
keeps automatically imported tokens in memory only. The choice is scoped to a non-reversible fingerprint of the
active API credential and saved-account slot, so replacing a key or switching accounts cannot silently reuse an old
browser session. Validation results are cached briefly so normal
refreshes do not probe every profile, and a temporary network failure does not erase a previously validated profile.
If the selected session expires, CodexBar asks before switching to another valid profile.

Browser-only balance refreshes also keep the last live balance and its original timestamp through temporary transport
failures when the failed request belongs to the same browser profile and token. This ownership proof stays in memory;
decoded snapshots, changed profiles or tokens, and API-key balances with optional browser enrichment cannot supply it.
Once DeepSeek rejects a session, a later network failure cannot preserve its old balance; successful validation must
restore that session first. A saved profile with unknown validity still supplies transport diagnostics when other profiles
succeed; a known-rejected selection keeps the profile picker and its valid alternatives.
Recognized transport failures still participate in startup retries when no matching balance can be retained. A browser-resolution
deadline without an observed session fails closed; cancelled or superseded refresh tasks keep their existing behavior.

If no session is valid, the menu keeps the API-key balance when one exists; otherwise it asks the user to sign in to
DeepSeek Platform in a Chromium-based browser. Authentication failures returned as top-level or nested DeepSeek codes
`40002` and `40003` are treated as expired sessions.

## Usage details

- The menu card shows total balance with the paid vs. granted breakdown:
  e.g. `$50.00 (Paid: $40.00 / Granted: $10.00)`.
- The API separates granted balance from topped-up balance; CodexBar labels these as granted vs. paid credit.
- With optional extra usage enabled, the menu shows today's and last-30-days cost and tokens,
  request counts, API-key count, the top model, per-model spend, a daily token chart, and a daily spend chart.
- Per-model spend uses the same reporting period and currency as detailed usage: **Last 30 days** for the preferred
  endpoints, or **This month** for monthly fallback. These are Platform-account totals across API keys. Missing or
  invalid model costs are omitted; a reported zero is retained.
- The amount and cost requests run concurrently. After balance arrives, CodexBar waits up to five seconds for
  automatic browser resolution and detailed usage. The deadline remains bounded even if a local browser read does not
  respond to cancellation. If the optional work fails or times out, the balance and previously validated profile list
  remain available while the menu reports that detailed usage is unavailable.
- With multiple configured API keys, browser-derived detailed usage is shown only with the active API-key account;
  other account cards remain balance-only so website usage is never duplicated across accounts.
- When multiple currencies are present, USD is shown preferentially.
- If total balance is zero, CodexBar shows an add-credits message. If balance is nonzero but `is_available` is false, it shows "Balance unavailable for API calls".
- There is no session or weekly window — DeepSeek does not expose per-window quota via API.
- Token-account selection injects the selected key into the fetch environment; otherwise CodexBar reads `DEEPSEEK_API_KEY` / `DEEPSEEK_KEY`.

## Peak / off-peak price clock

The menu card shows a countdown to the next pricing transition, the surrounding windows, and a
24-hour timeline. It is **display-only**: it never re-prices usage, and reported spend always comes
from DeepSeek's own cost reports.

Mirrored terms (`https://api-docs.deepseek.com/quick_start/pricing`, "Models & Pricing"), stated by
the vendor in Beijing time (UTC+8) and quoted here as UTC:

| Tier | Days | Beijing time | UTC |
|---|---|---|---|
| Peak | Mon–Fri | 09:00–12:00 | 01:00–04:00 |
| Off-peak | Mon–Fri | 12:00–14:00 | 04:00–06:00 |
| Peak | Mon–Fri | 14:00–18:00 | 06:00–10:00 |
| Off-peak | Mon–Fri | 18:00–09:00 | 10:00–01:00 next day |
| Off-peak | Sat–Sun | all day | Fri 10:00 → Mon 01:00 continuously |

Off-peak rates are 50% of peak rates. Not modelled: Chinese public holidays and adjusted make-up
workdays (both bill at off-peak), and the vendor assigns the tier from the moment its server receives
a request, which the docs do not define as start or completion time. The card states **Excludes
Chinese public holidays** directly under the tier — with the source and verification date on hover —
so the computed tier is never presented as a bare billing fact.

Maintenance — DeepSeek has revised these terms repeatedly, so treat a stale schedule as a defect:

1. Re-read the pricing page and compare it with the table above.
2. Update the peak hours in `DeepSeekPriceSchedule.transitions` and bump
   `DeepSeekPriceSchedule.termsLastVerifiedOn`.
3. Refresh the table in this section.
4. Run `swift test --filter DeepSeekPriceScheduleTests`; the Beijing-time test fails until the
   windows match the published terms.

What the user sees without hovering is the **Excludes Chinese public holidays** caption under the tier.
The source URL and the verification date are one hover away on the pricing badge, so a user who wants to
know when the mirrored terms were last checked — or where they came from — can find both there.

## Key files

- `Sources/CodexBarCore/Providers/DeepSeek/DeepSeekProviderDescriptor.swift` (descriptor + fetch strategy)
- `Sources/CodexBarCore/Providers/DeepSeek/DeepSeekUsageFetcher.swift` (HTTP client + JSON parser)
- `Sources/CodexBarCore/Providers/DeepSeek/DeepSeekPlatformTokenImporter.swift` (Chromium-family Platform session import)
- `Sources/CodexBarCore/Providers/DeepSeek/DeepSeekSettingsReader.swift` (env var resolution)
- `Sources/CodexBarCore/Providers/DeepSeek/DeepSeekPriceSchedule.swift` (peak/off-peak windows and their source of truth)
- `Sources/CodexBar/Providers/DeepSeek/DeepSeekProviderImplementation.swift` (provider activation and token-account visibility)
- `Sources/CodexBarCore/TokenAccountSupportCatalog+Data.swift` (DeepSeek token-account injection)
