"""Historical FX rates via Frankfurter (https://www.frankfurter.dev/).

Frankfurter is a free, no-key API serving ECB daily rates back to
1999. The response for `/{date}?from={ccy}&to=USD` looks like:

    {"amount": 1.0, "base": "INR", "date": "2024-09-02",
     "rates": {"USD": 0.01192}}

Limits we accept:
  * ECB only publishes business-day rates; Frankfurter auto-rounds
    requests for Saturdays/Sundays/holidays to the nearest prior
    business day (the response `date` reflects that). Good enough
    for Stanford reimbursement.
  * Frankfurter covers ~33 currencies (major + EUR area + emerging
    markets); some exotic codes (KZT, OMR, etc.) aren't supported.
    The fetcher returns `None` for unsupported codes; callers fall
    back to whatever mock/extracted rate is already on the line.
  * No API key, no auth header. ~200ms per request.

Used by `scripts/fx_enrich.py` (the pipeline post-reduce step) and
optionally by ad-hoc callers (acceptance harness, debug scripts).
"""

from __future__ import annotations

import json
import sys
import urllib.error
import urllib.request
from typing import Optional

from log_event import log_warning

BASE_URL = "https://api.frankfurter.dev/v1"
TIMEOUT_SEC = 8.0


def fetch_rate(
    currency: str,
    date: str,
    cache: Optional[dict[tuple[str, str], Optional[float]]] = None,
) -> Optional[float]:
    """Fetch USD-per-{currency} for the given date.

    `currency`: ISO 4217 3-letter code (e.g. 'INR', 'CHF', 'BRL').
    `date`: ISO 8601 'YYYY-MM-DD'.
    `cache`: optional dict mapping (currency_upper, date) → rate (or None
    for known-unsupported). Mutated in place when a real fetch happens.

    Returns:
      float — USD per 1 unit of `currency`, when Frankfurter answered.
      None — when the currency isn't covered, the API errored, or the
      response didn't contain the USD rate. Callers should keep
      whatever mock/extracted rate was already on the line and
      surface this as a workbench warning if the line is foreign.

    Special case: `currency == 'USD'` returns 1.0 without fetching.
    """
    ccy = currency.upper().strip()
    if ccy == "USD":
        return 1.0
    if not ccy or not date:
        return None

    if cache is not None:
        hit = cache.get((ccy, date))
        if (ccy, date) in cache:
            return hit

    url = f"{BASE_URL}/{date}?from={ccy}&to=USD"
    # Frankfurter's CDN (Cloudflare-fronted) rejects `Python-urllib/*`
    # User-Agent strings with a 403. Send a distinct UA so the request
    # is identifiable + acceptable.
    headers = {
        "Accept": "application/json",
        "User-Agent": "stanford-expense-reports/1.0 (+https://expense-reports-wgnivgelea-uw.a.run.app)",
    }
    try:
        req = urllib.request.Request(url, headers=headers)
        with urllib.request.urlopen(req, timeout=TIMEOUT_SEC) as resp:
            body = resp.read().decode("utf-8")
    except urllib.error.HTTPError as err:
        # 422: currency not supported. 404: date out of range. Either way,
        # no rate — record None in cache so we don't retry per-line.
        if cache is not None:
            cache[(ccy, date)] = None
        log_warning("fx.lookup.http_error", currency=ccy, date=date,
                    http_code=err.code)
        return None
    except (urllib.error.URLError, TimeoutError, OSError) as err:
        # Network failure: DO NOT cache (might come back next call).
        log_warning("fx.lookup.network_error", currency=ccy, date=date,
                    error_type=type(err).__name__, error=str(err)[:120])
        return None

    try:
        data = json.loads(body)
    except json.JSONDecodeError:
        log_warning("fx.lookup.bad_json", currency=ccy, date=date,
                    body_excerpt=body[:120])
        return None

    rate = data.get("rates", {}).get("USD")
    if not isinstance(rate, (int, float)) or rate <= 0:
        if cache is not None:
            cache[(ccy, date)] = None
        log_warning("fx.lookup.no_rate", currency=ccy, date=date)
        return None

    rate_f = float(rate)
    if cache is not None:
        cache[(ccy, date)] = rate_f
    return rate_f
