"""Download only the pinned checkpoint components used by the int8 preset."""
from pathlib import Path
import sys
from huggingface_hub import snapshot_download

root = Path(sys.argv[1])
for repo, revision, folder, patterns in [
    ('TensorForger/RIFE-safetensors', '78a62b7c2dd910536432d6c2c3a25e76f14fbf78',
     'RIFE-safetensors', ['flownet.safetensors']),
    ('black-forest-labs/FLUX.2-klein-4B', 'e7b7dc27f91deacad38e78976d1f2b499d76a294',
     'FLUX.2-klein-4B', ['scheduler/*', 'vae/*']),
    ('aydin99/FLUX.2-klein-4B-int8', 'f38839c94b597211dea83b1326f1bb4ce4142524',
     'FLUX.2-klein-4B-int8', ['config.json', 'quanto_qmap.json', 'diffusion_pytorch_model.safetensors',
                           'text_encoder/*', 'tokenizer/*']),
]:
    snapshot_download(repo, revision=revision, local_dir=root / folder,
                      allow_patterns=patterns, max_workers=3)
