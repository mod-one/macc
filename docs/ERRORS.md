# Error Catalog

This catalog defines error code naming and the initial web/API codes.

## Naming

Format: `MACC-<DOMAIN>-<4 digits>`

- `DOMAIN` examples: `WEB`, `CORE`, `COORD`.
- Codes are stable once published.

## Categories

The API error envelope uses these categories:

- `Validation`
- `Auth`
- `Dependency`
- `Conflict`
- `NotFound`
- `Internal`

## Web/API Codes

- `MACC-WEB-0000`: Unspecified web API failure (fallback when no specific mapping is available).
