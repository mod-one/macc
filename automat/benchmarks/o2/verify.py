"""Independent checks executed in a bounded subprocess, outside the AI fixture."""
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
from fixtures import verify

if __name__ == '__main__':
    checked = verify(Path(sys.argv[1]), sys.argv[2], int(sys.argv[3]))
    print(f'{checked} deterministic assertions passed')
