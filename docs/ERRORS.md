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

### Validation

- `MACC-WEB-0100`: Generic validation failure (`MaccError::Validation`).
- `MACC-WEB-0101`: Tool specification validation failure (`MaccError::ToolSpec`).
- `MACC-WEB-0102`: Secret scan validation failure (`MaccError::SecretDetected`).
- `MACC-WEB-0103`: Configuration parse/validation failure (`MaccError::Config`).
- `MACC-WEB-0104`: Catalog operation validation failure (`MaccError::Catalog`).

### Auth

- `MACC-WEB-0200`: User-scope operation denied in current mode (`MaccError::UserScopeNotAllowed`).

### NotFound

- `MACC-WEB-0300`: Project root cannot be resolved (`MaccError::ProjectRootNotFound`).
- `MACC-WEB-0301`: User home directory cannot be resolved (`MaccError::HomeDirNotFound`).

### Dependency

- `MACC-WEB-0400`: Local I/O dependency failed (`MaccError::Io`).
- `MACC-WEB-0401`: Remote fetch dependency failed (`MaccError::Fetch`).

### Internal

- `MACC-WEB-0500`: Coordinator workflow failure (`MaccError::Coordinator`).
- `MACC-WEB-0501`: Coordinator storage backend failure (`MaccError::Storage`).
- `MACC-WEB-0502`: Git subsystem failure (`MaccError::Git`).
