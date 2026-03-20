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

### Validation (1000 range)

- `MACC-WEB-1000`: Generic validation failure (`MaccError::Validation`).
- `MACC-WEB-1001`: Apply confirmation required before write execution.
- `MACC-WEB-1002`: Secret scan validation failure (`MaccError::SecretDetected`).
- `MACC-WEB-1003`: Configuration parse/validation failure (`MaccError::Config`).
- `MACC-WEB-1004`: Catalog operation validation failure (`MaccError::Catalog`).
- `MACC-WEB-1005`: Registry action payload or path validation failure.
- `MACC-WEB-1006`: Tool specification validation failure (`MaccError::ToolSpec`).

### NotFound (2000 range)

- `MACC-WEB-2000`: Project root cannot be resolved (`MaccError::ProjectRootNotFound`).
- `MACC-WEB-2001`: User home directory cannot be resolved (`MaccError::HomeDirNotFound`).
- `MACC-WEB-2002`: Registry task was not found for the requested operator action.
- `MACC-WEB-2003`: Backup set was not found for the requested restore action.

### Conflict / Auth (3000 range)

- `MACC-WEB-3000`: User-scope operation denied in current mode (`MaccError::UserScopeNotAllowed`).
- `MACC-WEB-3001`: Registry operator action conflicts with the task's current state/runtime.

### Dependency / Engine (4000 range)

- `MACC-WEB-4000`: Local I/O dependency failed (`MaccError::Io`).
- `MACC-WEB-4001`: Remote fetch dependency failed (`MaccError::Fetch`).

### Internal (5000 range)

- `MACC-WEB-5000`: Coordinator workflow failure (`MaccError::Coordinator`).
- `MACC-WEB-5001`: Coordinator storage backend failure (`MaccError::Storage`).
- `MACC-WEB-5002`: Git subsystem failure (`MaccError::Git`).
