#!/usr/bin/env python3
"""Private local regression: no network or Photos access; never edits samples.
Usage: python3 Tools/regress_private_samples.py [bundled-or-release-cli]
Writes fresh outputs under rust/target/apple-compat-validation-<timestamp>.
Run Tools/verify_imageio.swift separately for independent system decoding.
"""
from pathlib import Path
import hashlib,json,subprocess,sys,datetime,struct,os
ROOT=Path(__file__).resolve().parents[1]
sys.path.insert(0,str(ROOT/'Tools'))
from verify_burst import burst_info,sos_suffix
samples=Path(os.environ.get('MOMENTPORT_PRIVATE_SAMPLES', ROOT/'样片')).resolve()
if not (samples/'Honor').is_dir() or not (samples/'apple').is_dir():
 raise SystemExit('Private regression requires Honor/ and apple/ under MOMENTPORT_PRIVATE_SAMPLES (default: 样片/); samples are not included.')
cli=Path(sys.argv[1]).resolve() if len(sys.argv)>1 else ROOT/'MomentPort.app/Contents/Resources/momentport-cli'
out=ROOT/'rust/target'/('apple-compat-validation-'+datetime.datetime.now().strftime('%Y%m%d-%H%M%S'))
out.mkdir(parents=True)
def hashes():
 return {str(p.relative_to(samples)):hashlib.sha256(p.read_bytes()).hexdigest() for p in samples.rglob('*') if p.is_file()}
def run(input,output,*flags):
 r=subprocess.run([str(cli),str(input),str(output),*flags],capture_output=True,text=True,check=True)
 (output/'cli.log').write_text(r.stdout+r.stderr)
 report=json.loads((output/'MomentPortReport.json').read_text())
 expected=preview(input)['groups']
 assert len(report['groups'])==len(expected)
 for actual,group in zip(report['groups'],expected):
  assert actual['frameCount']==len(group['photos']), 'conversion skipped frames'
  manifest=json.loads((output/actual['folderName']/'BurstManifest.json').read_text())
  assert not manifest['skipped'], 'conversion skipped frames'
  assert manifest['keyFrameIndex']==group['keyIndex']
  assert manifest['keyFrameFileName']==group['photos'][group['keyIndex']]['fileName']
  assert sum(f['isKeyFrame'] for f in manifest['frames'])==1
 return report
def preview(input,*flags):
 return json.loads(subprocess.check_output([str(cli),str(input),'--list-json',*flags],text=True))
before=hashes()
p=preview(samples/'Honor')
assert [len(g['photos']) for g in p['groups']]==[3,25,4,7,7]
r=run(samples/'Honor',out/'all-46')
photo_ids=set()
for i,g in enumerate(p['groups'],1):
 folder=out/'all-46'/f'Burst_{i:02}'
 m=json.loads((folder/'BurstManifest.json').read_text())
 assert m['keyFrameIndex']==g['keyIndex']
 assert sum(f['isKeyFrame'] for f in m['frames'])==1
 bids=set()
 for f in folder.glob('*.JPG'):
  original=samples/'Honor'/g['photos'][0]['dirKey']/f.name
  assert original.exists(),str(original)
  bid,pid=burst_info(f)
  assert bid and pid and pid not in photo_ids
  bids.add(bid);photo_ids.add(pid)
  assert sos_suffix(f)==sos_suffix(original)
 assert len(bids)==1
assert len(photo_ids)==46
# Exclude covers using symlinks; the originals remain untouched.
for source_dir in sorted((samples/'Honor').iterdir()):
 if not source_dir.is_dir(): continue
 dest=out/'no-cover-input'/source_dir.name;dest.mkdir(parents=True)
 for f in source_dir.glob('*.JPG'):
  if '_COVER' not in f.stem: (dest/f.name).symlink_to(f)
p2=preview(out/'no-cover-input')
assert [len(g['photos']) for g in p2['groups']]==[3,24,3,6,7]
assert all(g['keyIndex']==0 for g in p2['groups'])
run(out/'no-cover-input',out/'no-cover-43')
# Few-frame manual test set: 3 frames with a real Honor cover, and 3 without.
for name,frames in [('without-cover',p['groups'][0]['photos']),('with-cover',p['groups'][1]['photos'][:3])]:
 dest=out/'manual-input'/name;dest.mkdir(parents=True)
 for f in frames: (dest/f['fileName']).symlink_to(samples/'Honor'/f['dirKey']/f['fileName'])
run(out/'manual-input',out/'manual-6')
# Both outer TIFF orders with real JPEG pixels, minimal unambiguous EXIF.
f=next((samples/'Honor/1').glob('*.JPG'));d=f.read_bytes()
length=int.from_bytes(d[4:6],'big');assert d[2:4]==b'\xff\xe1' and d[6:12]==b'Exif\0\0'
for en,name in [('<','little'),('>','big')]:
 dest=out/'byte-order-input'/name;dest.mkdir(parents=True)
 # Header(8), IFD0 count(2), Make entry(12), nextIFD(4), string(6).
 t=(b'II' if en=='<' else b'MM')+struct.pack(en+'HIHHHI',42,8,1,271,2,6)+struct.pack(en+'II',26,0)+b'HONOR\0'
 payload=b'Exif\0\0'+t
 jpeg=d[:2]+b'\xff\xe1'+struct.pack('>H',len(payload)+2)+payload+d[4+length:]
 for n in range(2): (dest/f'{name}-{n}.JPG').write_bytes(jpeg)
run(out/'byte-order-input',out/'byte-order-4')
assert before==hashes(),'original files changed'
(out/'original-sha256.json').write_text(json.dumps(before,indent=2))
(out/'results.json').write_text(json.dumps({'honorFrames':46,'groupSizes':[3,25,4,7,7],'withoutCoverSizes':[3,24,3,6,7],'originalFileCount':len(before),'originalsUnchanged':True,'photosGroupingVerified':False},indent=2))
print('PASS: 46 real frames, 5 groups, covers/default keys, no-cover 43, manual 6, both outer TIFF orders, SOS and original SHA-256 unchanged')
print(out)
