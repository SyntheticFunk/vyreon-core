# External checks

Published values against a source that is neither the engine nor Intrinio. A check here is a band
or a convergence statement **declared and committed before the values are compared**, never fitted
to them. A band that fails is reported as failed; changing a band after seeing values needs a
written reason and the owner.

## Cboe volatility indices (`cboe.py`)

Cboe's daily closes, read 2026-11-18 from
`https://cdn.cboe.com/api/global/us_indices/daily_prices/<INDEX>_History.csv` and frozen in
`cboe/` (`SHA256SUMS`). Each index is Cboe's model-free 30-day implied volatility (the VIX method:
the whole out-of-the-money strip, i.e. a 30-day variance-swap rate). Compared with the engine's
base-layer `VS-IVS01` role `iv30` (the fitted surface's at-the-money 30-day IV) for:

| Index | Cboe's underlying | Our security | Same options? |
|---|---|---|---|
| VIX | S&P 500 index (SPX) options | SPY | no (SPX vs SPY) |
| VXN | Nasdaq-100 index (NDX) options | QQQ | no (NDX vs QQQ) |
| RVX | Russell 2000 index (RUT) options | IWM | no (RUT vs IWM) |
| GVZ | GLD options | GLD | yes |
| OVX | USO options | USO | yes |

Why the index sits above our value: a variance-swap rate integrates the whole smile, so with a
convex smile it exceeds at-the-money IV (Carr and Wu, "Variance Risk Premiums", 2009; Cboe VIX
white paper). The gap widens with skew and with the level of volatility.

**Bands (declared 2026-11-18, before any sample value was compared):**

1. Per session: `-1 <= index - 100*iv30 <= 8` points for every pair. The upper 8 is the VIX band
   declared 2026-10-21 in the engine's `refcheck.outside`, kept unchanged; the same for VXN and RVX
   (equity-index smiles of the same shape) and for GVZ and OVX (same options, milder smile for gold,
   strongly convex for oil). The lower bound is -1, not the engine's 0: at-the-money conventions
   (spot vs forward), close times and interpolation can put a flat-smile pair slightly below.
2. Across the panel: the index and `100*iv30` correlate at >= 0.9 (the 2026-10-21 statement),
   judged only when the index's range over the panel is at least 3 points (a narrower range makes
   correlation mostly noise); otherwise reported, not judged.
3. A withheld `iv30` is reported with its state and reason, never compared.

Pass = every compared session inside band 1, and band 2 where judged.

## SEC EDGAR (`edgar.py`)

To be declared before its comparison runs.
