from PIL import Image, ImageEnhance
src = r"E:\ai\Projects\Meridian\meridian_printwindow.png"
im = Image.open(src).convert("RGB")
w, h = im.size
# Top-right quadrant: rightmost 45% width, top 60px strip
strip = im.crop((int(w * 0.55), 0, w, 60))
# Upscale 3x and boost contrast so faint 50%-opacity icons become visible
strip = strip.resize((strip.width * 3, strip.height * 3), Image.LANCZOS)
strip = ImageEnhance.Contrast(strip).enhance(2.2)
strip = ImageEnhance.Brightness(strip).enhance(0.8)
out = r"E:\ai\Projects\Meridian\meridian_toolbar_crop.png"
strip.save(out)
print("SAVED", out, strip.size)
