"""GeometryView: one image renderer for harmonic geometry arrays.

display/layout selects a labeled dashboard or a transparent RGBA image.
Raw field data stays in the input array; shaders read it directly.
Wire BioColors.rgb to palette to share its colors."""

import json
from fractions import Fraction
import numpy as np
from PIL import Image, ImageDraw, ImageFont
import goofi
from goofi.geometry import decode


INK = (11, 20, 32)
MUTED = (142, 162, 179)
PAPER = (233, 239, 234)
ACCENT = (78, 217, 194)
DEFAULT_PALETTE = np.array([[0.07, 0.19, 0.29], [0.12, 0.69, 0.68], [0.97, 0.70, 0.36], [0.98, 0.95, 0.80]])


def palette_at(t, colors):
    t = np.clip(np.asarray(t), 0, 1)*(len(colors)-1)
    lower = np.minimum(t.astype(int), len(colors)-2)
    return colors[lower]*(1-(t-lower)[..., None])+colors[lower+1]*(t-lower)[..., None]


class GeometryView(goofi.Node):
    """Render geometry, camera controls, harmonic components, and measurements."""

    TAGS = ["image"]
    INPUTS = {"input": goofi.InputSlot(goofi.DataType.ARRAY, required=True),
              "harmonic": goofi.InputSlot(goofi.DataType.ARRAY, required=False),
              "metrics": goofi.InputSlot(goofi.DataType.ARRAY, required=False),
              "palette": goofi.InputSlot(goofi.DataType.ARRAY, required=False)}
    OUTPUTS = {"image": goofi.DataType.ARRAY}
    PARAMS = {
        "display": {
            "layout": goofi.StringParam("dashboard", ["dashboard", "image"], doc="One rendered output: labeled dashboard or transparent geometry image."),
            "title": goofi.StringParam("Harmonic geometry", doc="Dashboard title."),
            "caption": goofi.StringParam("One harmonic frame. Many ways to see it.", doc="Short dashboard subtitle."),
            "size": goofi.IntParam(512, 128, 1024, doc="Transparent image side length; dashboard is 960 by 660."),
            "thickness": goofi.IntParam(2, 1, 5, doc="Curve and edge width in pixels."),
            "points": goofi.IntParam(3, 1, 8, doc="Radius of point-cloud and graph vertices."),
        },
        "camera": {
            "yaw": goofi.FloatParam(0.55, -1000.0, 1000.0, doc="3D camera rotation around the vertical axis, in radians."),
            "pitch": goofi.FloatParam(0.35, -3.14, 3.14, doc="3D camera tilt, in radians."),
            "zoom": goofi.FloatParam(0.9, 0.1, 4.0, doc="Scale of the drawing within the image."),
            "autofit": goofi.BoolParam(True, doc="Fit the coordinate extent; off uses camera/radius as a fixed extent."),
            "radius": goofi.FloatParam(1.0, 0.01, 1000.0, doc="Fixed coordinate radius when autofit is off."),
            "perspective": goofi.FloatParam(0.2, 0.0, 0.7, doc="Perspective depth; zero is orthographic."),
        },
        "field": {
            "style": goofi.StringParam("signed", ["signed", "magnitude", "nodal", "contours"], doc="Field coloring. Nodal reveals zero-displacement lines."),
            "range": goofi.FloatParam(1.0, 0.0, 1000.0, doc="Fixed absolute field range; 0 fits each frame and can change brightness."),
            "width": goofi.FloatParam(0.055, 0.001, 0.5, doc="Nodal line width relative to field range."),
        },
        "common": {"max_frequency": goofi.FloatParam(8.0, 0.0, 30.0)},
    }

    def setup(self):
        def font(size):
            for path in ("DejaVuSans.ttf", "C:/Windows/Fonts/segoeui.ttf"):
                try:
                    return ImageFont.truetype(path, size)
                except OSError:
                    pass
            return ImageFont.load_default(size=size)
        self.small, self.body, self.title = font(14), font(19), font(32)

    def process(self, input, harmonic=None, metrics=None, palette=None):
        table, p, cam = decode(input.data, input.meta), self.params.display, self.params.camera
        kind = table["type"]
        info = json.loads(table["info"])
        metadata = info.get("metadata", {})
        method = str(metadata.get("method", metadata.get("kind", kind)))
        coords = table["coordinates"]
        parts = coords if isinstance(coords, list) else [coords]
        colors = DEFAULT_PALETTE
        if palette is not None:
            pc = np.asarray(palette.data, dtype=np.float64)
            if pc.ndim < 2 or pc.shape[-1] != 3:
                raise ValueError("palette needs RGB rows, for example BioColors.rgb")
            pc = pc.reshape(-1, 3)
            pc = pc[np.all(np.isfinite(pc), axis=1)]
            if len(pc):
                colors = np.clip(pc, 0, 1)
                if len(colors) == 1:
                    colors = np.vstack((colors*0.15, colors))
        canvas = Image.new("RGBA", (p.size, p.size))
        if kind in ("field_2d", "vector_field_2d"):
            values = np.asarray(parts[0], dtype=np.float64)
            valid = np.isfinite(values) if values.ndim == 2 else np.all(np.isfinite(values), axis=-1)
            cover = valid.astype(np.float32)
            if "coverage" in table:
                cov = table["coverage"]
                if cov.shape != cover.shape or not np.all(np.isfinite(cov)):
                    raise ValueError("Geometry coverage must be finite and match the field")
                cover *= np.clip(cov, 0, 1)
            scalar = values if values.ndim == 2 else np.linalg.norm(values, axis=-1)
            safe = np.where(valid, scalar, 0)
            extent = self.params.field.range or max(np.max(np.abs(safe)), 1e-12)
            z = safe/extent
            style = self.params.field.style
            if style == "nodal":
                tone = np.exp(-0.5*(z/self.params.field.width)**2)
            elif style == "magnitude":
                tone = np.abs(z)
            elif style == "contours":
                tone = (0.5+0.5*np.cos(z*np.pi*12))**8
            else:
                tone = 0.5+0.5*z
            rgba = np.concatenate((palette_at(tone, colors), cover[..., None]), axis=-1)
            canvas = Image.fromarray(np.uint8(np.clip(rgba, 0, 1)*255)).resize((p.size, p.size), Image.Resampling.BILINEAR)
        else:
            self._draw_geometry(canvas, parts, table, kind, colors)
        if p.layout == "image":
            return {"image": (np.asarray(canvas, dtype=np.float32)/255, {"geometry_kind": method})}
        dashboard = Image.new("RGB", (960, 660), INK)
        draw = ImageDraw.Draw(dashboard)
        draw.rounded_rectangle((22, 22, 938, 638), radius=20, outline=(38, 59, 74), width=1)
        draw.text((44, 37), "GOOFI  /  HARMONIC GEOMETRY", font=self.small, fill=ACCENT)
        draw.text((42, 62), p.title[:43], font=self.title, fill=PAPER)
        draw.text((44, 107), p.caption[:98], font=self.small, fill=MUTED)
        draw.line((44, 138, 916, 138), fill=(38, 59, 74), width=1)
        draw.rounded_rectangle((42, 154, 620, 610), radius=12, fill=(6, 13, 22))
        display = canvas.resize((444, 444), Image.Resampling.LANCZOS)
        dashboard.paste(display, (109, 160), display)
        draw.text((649, 158), "FORM", font=self.small, fill=ACCENT)
        draw.text((649, 181), method.replace("_", " ")[:25], font=self.body, fill=PAPER)
        draw.text((649, 210), kind.replace("_", " "), font=self.small, fill=MUTED)
        draw.text((649, 249), "HARMONIC COMPONENTS", font=self.small, fill=ACCENT)
        if harmonic is not None:
            r, weights = np.asarray(harmonic.data)[:2]
            if r.ndim != 1 or r.shape != weights.shape or not np.all(np.isfinite(np.r_[r, weights])):
                raise ValueError("Dashboard harmonic context needs aligned finite ratios and amplitudes")
            maximum = max(float(weights.max()) if weights.size else 0, 1e-12)
            for i, (ratio, weight) in enumerate(zip(r[:8], weights[:8])):
                y = 280+i*24
                near = Fraction(float(ratio)).limit_denominator(16)
                label = str(near) if abs(float(near)-ratio) < 0.0001 else f"{ratio:.4f}"
                draw.text((649, y), label, font=self.small, fill=PAPER if weight > 0 else MUTED)
                draw.rounded_rectangle((727, y+4, 904, y+12), radius=3, fill=(27, 43, 57))
                width = int(177*weight/maximum)
                if width > 0:
                    color = tuple(np.uint8(palette_at(i/max(len(r)-1, 1), colors)*255))
                    draw.rounded_rectangle((727, y+4, 727+width, y+12), radius=3, fill=color)
            if len(r) > 8:
                draw.text((649, 478), f"+ {len(r)-8} components", font=self.small, fill=MUTED)
            t = float(harmonic.meta.get("morph", 0))
            draw.text((649, 509), f"MORPH  {t:.3f}", font=self.small, fill=ACCENT)
            draw.line((649, 540, 904, 540), fill=(44, 62, 77), width=3)
            at = 649+255*np.clip(t, 0, 1)
            draw.ellipse((at-5, 535, at+5, 545), fill=ACCENT)
        else:
            draw.text((649, 282), "Two endpoint geometries" if method == "geometry_blend" else "Geometry frame", font=self.body, fill=PAPER)
            draw.text((649, 316), "Mix moves between their shapes." if method == "geometry_blend" else "Add harmonic context with a cable.", font=self.small, fill=MUTED)
            if "morph" in metadata:
                t = float(metadata["morph"])
                draw.text((649, 509), f"MORPH  {t:.3f}", font=self.small, fill=ACCENT)
                draw.line((649, 540, 904, 540), fill=(44, 62, 77), width=3)
                at = 649+255*np.clip(t, 0, 1)
                draw.ellipse((at-5, 535, at+5, 545), fill=ACCENT)
        if metrics is not None:
            numeric = []
            names = metrics.meta.get("channels", {}).get("dim0", [])
            for key, value in zip(names, np.asarray(metrics.data).ravel()):
                if np.isfinite(value):
                    numeric.append((key, float(value)))
            text = "   |   ".join(f"{k.replace('_', ' ')} {v:.3g}" for k, v in numeric[:3])
            draw.text((48, 617), text[:115], font=self.small, fill=MUTED)
        else:
            draw.text((48, 617), "Geometry is data. Color, camera, sound, and motion can share the same source.", font=self.small, fill=MUTED)
        if not any(part.size for part in parts):
            draw.text((229, 366), "No harmonic components", font=self.body, fill=MUTED)
        meta = {"geometry_kind": method}
        return {"image": (np.asarray(dashboard, dtype=np.float32)/255, meta)}

    def _draw_geometry(self, canvas, parts, table, kind, colors):
        p, c = self.params.display, self.params.camera
        good = [np.asarray(part, dtype=np.float64) for part in parts if part.size]
        if not good:
            return
        if any(part.ndim != 2 or part.shape[1] not in (2, 3) or not np.all(np.isfinite(part)) for part in good):
            raise ValueError("Drawable geometry needs finite Nx2 or Nx3 coordinates")
        pts = [np.pad(part, ((0, 0), (0, 3-part.shape[1]))) for part in good]
        all_points = np.vstack(pts)
        center = (all_points.min(axis=0)+all_points.max(axis=0))/2 if c.autofit else np.zeros(3)
        radius = max(np.linalg.norm(all_points-center, axis=1).max(), 1e-9) if c.autofit else c.radius
        cy, sy, cp, sp = np.cos(c.yaw), np.sin(c.yaw), np.cos(c.pitch), np.sin(c.pitch)
        rotation = np.array([[cy, 0, sy], [sp*sy, cp, -sp*cy], [-cp*sy, sp, cp*cy]])
        is3d = any(part.shape[1] == 3 for part in good)
        transformed = [((part-center)/radius) @ rotation.T if is3d else (part-center)/radius for part in pts]
        projected = [part[:, :2]/np.maximum(1-c.perspective*part[:, 2:3], 0.2) for part in transformed]
        projected = [part*np.array([1, -1])*p.size*0.44*c.zoom + p.size/2 for part in projected]
        draw = ImageDraw.Draw(canvas)
        color = lambda t, alpha=255: (*tuple(np.uint8(palette_at(t, colors)*255)), alpha)
        first = projected[0]
        if kind == "mesh_3d" and "faces" in table:
            raw = table["faces"]
            if raw.ndim != 2 or raw.shape[1] != 3 or not np.all(np.isfinite(raw)) or np.any(raw != np.floor(raw)) or np.any(raw < 0) or np.any(raw >= len(first)):
                raise ValueError("Mesh faces must contain valid triangle indices")
            faces = raw.astype(int)
            xyz = transformed[0]
            normals = np.cross(xyz[faces[:, 1]]-xyz[faces[:, 0]], xyz[faces[:, 2]]-xyz[faces[:, 0]])
            normals /= np.maximum(np.linalg.norm(normals, axis=1, keepdims=True), 1e-12)
            light = np.clip(normals @ np.array([0.3, 0.5, 0.8]), -1, 1)
            for index in np.argsort(xyz[faces, 2].mean(axis=1)):
                triangle = faces[index]
                draw.polygon([tuple(v) for v in first[triangle]], fill=color(0.2+0.8*abs(light[index])))
        elif kind in ("graph", "tree") and "edges" in table:
            raw = table["edges"]
            if raw.ndim != 2 or raw.shape[1] != 2 or not np.all(np.isfinite(raw)) or np.any(raw != np.floor(raw)) or np.any(raw < 0) or np.any(raw >= len(first)):
                raise ValueError("Graph edges must contain valid vertex indices")
            weights = table["weights"].ravel() if "weights" in table else np.empty(0)
            weighted = len(weights) == len(raw) and np.all(np.isfinite(weights))
            scale = max(np.max(np.abs(weights)), 1e-12) if weighted and len(weights) else 1.0
            for i, edge in enumerate(raw.astype(int)):
                tone = float(np.clip(abs(weights[i])/scale, 0, 1)) if weighted else i/max(len(raw)-1, 1)
                draw.line([tuple(v) for v in first[edge]], fill=color(0.25+0.7*tone, int(90+150*tone)), width=p.thickness)
            if len(first) <= 256:
                for i, (x, y) in enumerate(first):
                    draw.ellipse((x-p.points, y-p.points, x+p.points, y+p.points), fill=color(0.9))
        elif kind.startswith("point_cloud"):
            weights = table["weights"].ravel() if "weights" in table else np.ones(len(first))
            for i in np.argsort(transformed[0][:, 2]):
                if len(weights) == len(first) and weights[i] <= 0:
                    continue
                x, y = first[i]
                size = p.points if len(first) < 200 else max(1, p.points-1)
                draw.ellipse((x-size, y-size, x+size, y+size), fill=color(i/max(len(first)-1, 1), 225))
        else:
            for index, points in enumerate(projected):
                if kind in ("polygon", "polygon_set"):
                    points = np.vstack((points, points[:1]))
                for i in range(len(points)-1):
                    t = i/max(len(points)-2, 1) if len(projected) == 1 else index/max(len(projected)-1, 1)
                    draw.line([tuple(points[i]), tuple(points[i+1])], fill=color(0.25+0.75*t), width=p.thickness)
