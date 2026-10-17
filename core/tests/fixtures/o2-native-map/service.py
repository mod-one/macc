class User:
    def __init__(self, identifier: int):
        self.identifier = identifier
class Repository:
    def lookup(self, identifier: int) -> User | None:
        return User(identifier)
    async def refresh(self) -> None:
        self.lookup(1)
def trace(operation):
    return operation
@trace
def sync() -> bool:
    return True
EXAMPLE = "def lookup is documentation, not a declaration"
