"""Download and verify the optional RealESRGAN checkpoint."""
import argparse
import hashlib
from pathlib import Path
import urllib.request

URL = 'https://github.com/xinntao/Real-ESRGAN/releases/download/v0.2.5.0/realesr-general-x4v3.pth'
SHA256 = '8dc7edb9ac80ccdc30c3a5dca6616509367f05fbc184ad95b731f05bece96292'


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--output', required=True, type=Path, help='Checkpoint file path')
    args = parser.parse_args()
    path = args.output.absolute()
    path.parent.mkdir(parents=True, exist_ok=True)
    if not path.exists():
        temporary = path.with_suffix('.download')
        try:
            urllib.request.urlretrieve(URL, temporary)
            if hashlib.sha256(temporary.read_bytes()).hexdigest() != SHA256:
                raise ValueError('RealESRGAN checkpoint checksum mismatch')
            temporary.replace(path)
        finally:
            temporary.unlink(missing_ok=True)
    if hashlib.sha256(path.read_bytes()).hexdigest() != SHA256:
        raise ValueError('Existing file is not the expected RealESRGAN checkpoint')
    print(f'Set RealESRGAN model/weights to: {path}')


if __name__ == '__main__':
    main()
