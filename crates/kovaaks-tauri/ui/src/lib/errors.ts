/**
 * Maps raw backend error strings (kovaaks-core `Error` Display output plus
 * plain Tauri command strings) to human-friendly messages. Anything
 * unrecognized falls through unchanged so details are never silently lost.
 */
function rawMessage(err: unknown): string {
  if (err instanceof Error) return err.message
  return String(err)
}

export function humanError(err: unknown): string {
  const msg = rawMessage(err)
  // Error::InvalidSteamId / Error::SteamNotFound from the evxl resolver.
  if (/invalid steam id|steam identifier not found/i.test(msg)) {
    return "That doesn't look like a Steam ID, vanity URL, or profile link"
  }
  // Error::RateLimited — 429/5xx after the engine's retries were exhausted.
  if (/rate limited|server unavailable/i.test(msg)) {
    return "KovaaK's API is rate-limiting us — retrying shortly"
  }
  // Error::Http wraps every reqwest failure (DNS, refused, timeout, TLS).
  if (/http request failed|error sending request|network|timed out|connection/i.test(msg)) {
    return 'Network error — check your connection'
  }
  return msg
}
