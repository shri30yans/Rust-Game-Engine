# Re-render hero still and/or preview MP4 from a saved <weapon>.blend (written by run_pipeline)
#   blender -b <weapon>.blend -P render_from_blend.py -- <weapon> [hero] [preview]
import sys, os, json
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import wkit
from wkit import *
args = sys.argv[sys.argv.index('--') + 1:]
W = args[0]; what = set(args[1:]) or {'hero', 'preview'}
sc = bpy.context.scene
wkit.CLIPS[:] = [tuple(c) for c in json.loads(sc['wk_clips'])]
wkit.ANIM[:] = [bpy.data.objects[n] for n in json.loads(sc['wk_anim'])]
opts = json.loads(sc.get('wk_opts', '{}'))
root = bpy.data.objects[opts['root']]
meshes = [bpy.data.objects[n] for n in opts['meshes'] + opts['fx']]
studio(meshes, accent=tuple(opts['accent']), cam_dir=tuple(opts['cam_dir']), lens=opts['lens'])
rdir = os.path.join(WROOT, 'renders')
if 'hero' in what:
    render_still(os.path.join(rdir, f'{W}_hero.png'), int(os.environ.get('HERO_SAMPLES', '128')))
if 'preview' in what:
    total, labels = build_preview_timeline(root, opts['seq'])
    render_preview(os.path.join(rdir, f'{W}_preview.mp4'), os.path.join(WROOT, 'blender', W, 'preview_frames'), labels,
                   samples=int(os.environ.get('PREVIEW_SAMPLES', '8')))
