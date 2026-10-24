import sys
from PIL import Image, ImageChops
a, b = Image.open(sys.argv[1]).convert("RGB"), Image.open(sys.argv[2]).convert("RGB")
assert a.size == b.size, (a.size, b.size)
d = ImageChops.difference(a, b)
bbox = d.getbbox()
px = sum(1 for p in d.getdata() if p != (0, 0, 0))
mask_h = int(sys.argv[4]) if len(sys.argv) > 4 else 0  # 忽略最底下 mask_h px（狀態列 FPS/RSS 每秒變）
W, H = a.size
px_masked = sum(1 for i, p in enumerate(d.getdata()) if p != (0, 0, 0) and i // W < H - mask_h)
print(f"diff_pixels={px} of {W*H}  bbox={bbox}  diff_pixels_excl_bottom{mask_h}px={px_masked}")
if len(sys.argv) > 3 and sys.argv[3] and bbox:
    d.point(lambda v: 255 if v else 0).save(sys.argv[3])
