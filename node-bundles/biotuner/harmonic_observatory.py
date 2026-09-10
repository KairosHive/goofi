import time
from collections import deque
import numpy as np
from PIL import Image, ImageDraw, ImageFont
import goofi


def activation_rgb(values, ceiling):
    z = np.clip(np.log1p(np.maximum(values, 0)/max(ceiling, 1e-12)*1000)/np.log(1001), 0, 1)
    stops = np.array([[8, 17, 32], [21, 69, 92], [30, 161, 164], [245, 194, 105], [255, 246, 219]])
    u = z*4
    k = np.minimum(u.astype(int), 3)
    return (stops[k]*(1-(u-k)[..., None])+stops[k+1]*(u-k)[..., None]).astype(np.uint8)


class HarmonicObservatory(goofi.Node):
    """An evolving atlas of power-weighted harmonic activation and history.

    Activation is supplied by HarmonicSpectrum: kernel times normalized power
    outer product. Color uses a labelled logarithmic scale with a slowly
    decaying peak ceiling. Matrix values themselves are never rescaled.

    Connect HarmonicSpectrum.analysis to analysis. All panels use one window.
    display/channel selects a named channel, or a row number for unlabeled data.
    Refresh the channel list after connecting a source or changing its channels.
    Connect dashboard to graphics:SignalIn in texture mode, or an image viewer.
    """
    TAGS = ["image"]
    INPUTS = {"analysis": goofi.InputSlot(goofi.DataType.TABLE, required=True)}
    OUTPUTS = {"dashboard": goofi.DataType.ARRAY}
    PARAMS = {"display": {"channel": goofi.StringParam("First channel", ["First channel"], refresh=True,
                        doc="Channel to display. Refresh the list after connecting; unlabeled channels use row numbers."),
                        "matrix_ceiling": goofi.FloatParam(0.0, 0.0, 100.0,
                        doc="0 tracks a slowly decaying peak; positive fixes the color ceiling.")},
              "common": {"max_frequency": goofi.FloatParam(6.0, 0.0, 30.0)}}

    def setup(self):
        def font(size):
            for name in ("DejaVuSans.ttf", "C:/Windows/Fonts/segoeui.ttf"):
                try:
                    return ImageFont.truetype(name, size)
                except OSError:
                    pass
            return ImageFont.load_default(size=size)
        self.small, self.body, self.title, self.big = font(16), font(21), font(34), font(29)
        self.history = deque(maxlen=180)
        self.trends = deque(maxlen=180)
        self.last_frame = None
        self.ceiling = 0.0
        self.text_cache = {}
        self.last_grid = None
        self.channel_names = []

    def refresh_display_channel(self):
        return ["First channel", *self.channel_names]

    def process(self, analysis):
        fields = analysis.table
        required = ("spectrum", "freqs", "activation", "peaks", "harmonicity", "complexity", "power", "waveform")
        missing = [name for name in required if name not in fields]
        if missing:
            raise ValueError("Connect HarmonicSpectrum.analysis; missing fields: " + ", ".join(missing))
        if any(fields[name].kind != "ARRAY" for name in required):
            raise ValueError("HarmonicObservatory analysis fields must be arrays")
        spectrum, freqs, matrix, peaks, harmonicity, complexity, power, waveform = (fields[name] for name in required)
        wave_shape = waveform.data.shape
        if not wave_shape or not all(wave_shape):
            raise ValueError("HarmonicObservatory needs a nonempty waveform")
        leading = wave_shape[:-1]
        axes = waveform.meta.get("channels", {})
        names = []
        has_labels = any(len(axes.get(f"dim{axis}", [])) == size for axis, size in enumerate(leading))
        for i, indices in enumerate(np.ndindex(leading)):
            parts = []
            for axis, index in enumerate(indices):
                labels = axes.get(f"dim{axis}", [])
                parts.append(str(labels[index]) if len(labels) == leading[axis] else str(index + 1))
            names.append(" / ".join(parts) if has_labels else f"Row {i + 1}")
        self.channel_names = [name if names.count(name) == 1 and name != "First channel"
                              else f"{name} [row {i + 1}]" for i, name in enumerate(names)]
        choice = self.params.display.channel
        if choice == "First channel":
            row = 0
        elif choice in self.channel_names:
            row = self.channel_names.index(choice)
        else:
            raise ValueError(f"Channel '{choice}' is not available. Refresh display/channel to choose from: "
                             + ", ".join(self.channel_names))
        channel = self.channel_names[row]
        image_meta = {"channel": channel}

        def selected(frame, tail):
            values = np.asarray(frame.data, dtype=float)
            if values.ndim < tail:
                raise ValueError("HarmonicObservatory needs the outputs of HarmonicSpectrum")
            shape = values.shape[-tail:]
            count = int(np.prod(values.shape[:-tail]))
            if values.shape[:-tail] != leading:
                raise ValueError("HarmonicObservatory analysis fields must have matching channel axes")
            return values.reshape((count,) + shape)[row]

        s = selected(spectrum, 1)
        f = np.asarray(freqs.data, dtype=float).ravel()
        p = selected(power, 1)
        a = selected(matrix, 2)
        c = selected(complexity, 1)
        pk = selected(peaks, 1)
        wave = selected(waveform, 1)
        means = np.asarray(harmonicity.data).ravel()
        if means.size != len(names):
            raise ValueError("HarmonicObservatory harmonicity must match the channel count")
        mean = float(means[row])
        if len(f) < 2 or s.shape != f.shape or p.shape != f.shape or a.shape != (len(f), len(f)):
            raise ValueError("HarmonicObservatory analysis fields must use the same frequency grid")
        if len(c) != 4 or not len(wave):
            raise ValueError("HarmonicObservatory needs four complexity values and a nonempty waveform")
        sfreq = waveform.meta.get("sfreq")
        if sfreq is None or not np.isfinite(sfreq) or sfreq <= 0:
            raise ValueError("HarmonicObservatory waveform needs finite, positive sfreq metadata")
        if not np.all(np.isfinite(f)) or np.any(np.diff(f) <= 0):
            raise ValueError("HarmonicObservatory frequencies must be finite and increasing")
        if not all(np.all(np.isfinite(v)) for v in (s, p, a, wave)):
            self.history.clear()
            self.trends.clear()
            self.last_frame = None
            self.ceiling = 0.0
            im = Image.new("RGB", (825, 629), (7, 12, 23))
            ImageDraw.Draw(im).text((24, 24), "No valid spectrum for the selected row", font=self.body, fill=(233, 241, 250))
            return {"dashboard": (np.asarray(im).astype(np.float32) / 255, image_meta)}
        a = np.maximum(a, 0)
        grid = (channel, row, tuple(f))
        if grid != self.last_grid:
            self.history.clear()
            self.trends.clear()
            self.last_frame = None
            self.ceiling = 0.0
            self.last_grid = grid
        now = time.monotonic()
        stamp = spectrum.meta.get("index", spectrum.meta.get("time"))
        if stamp != self.last_frame or stamp is None:
            if self.history and len(self.history[-1][1]) != len(s):
                self.history.clear()
            self.history.append((now, s.copy()))
            self.trends.append((now, mean, float(c[1]), float(c[0])))
            self.last_frame = stamp
        self.ceiling = max(float(a.max()), self.ceiling*0.997, 1e-9)
        ceiling = self.params.display.matrix_ceiling or self.ceiling
        im = Image.new("RGB", (1600, 1220), (7, 12, 23))
        d = ImageDraw.Draw(im)
        teal, gold, muted, white = (79, 231, 212), (255, 191, 108), (137, 158, 181), (233, 241, 250)
        def label(x, y, text, color=muted, font=None):
            font = font or self.small
            key = (text, color, id(font))
            cached = self.text_cache.get(key)
            if cached is None:
                left, top, right, bottom = font.getbbox(text)
                tile = Image.new("RGBA", (max(1, right-left), max(1, bottom-top)))
                ImageDraw.Draw(tile).text((-left, -top), text, font=font, fill=color)
                cached = (tile, left, top)
                if len(self.text_cache) >= 4096:
                    self.text_cache.clear()
                self.text_cache[key] = cached
            tile, left, top = cached
            im.paste(tile, (int(x+left), int(y+top)), tile)
        def card(box, title):
            d.rounded_rectangle(box, radius=12, fill=(12, 21, 36), outline=(29, 45, 63))
            label(box[0]+18, box[1]+13, title, teal)
        def plot(values, box, color, lo=None, hi=None):
            values = np.nan_to_num(np.asarray(values, dtype=float))
            lo = float(values.min()) if lo is None else lo
            hi = float(values.max()) if hi is None else hi
            x0,y0,x1,y1 = box
            for frac in (0, .5, 1):
                yy = y1-(y1-y0)*frac
                d.line((x0,yy,x1,yy), fill=(28,42,58))
            norm = np.clip((values-lo)/max(hi-lo,1e-12),0,1)
            xy = list(zip(np.linspace(x0,x1,len(values)),y1-norm*(y1-y0)))
            if len(xy)>1:
                d.line(xy, fill=color, width=2)
        label(28, 17, "HARMONIC OBSERVATORY", white, self.title)
        label(30, 65, "Harmonic spectrum  /  power-weighted activation  /  complexity  /  history")
        label(1120, 24, f"CHANNEL: {channel}", gold)

        card((24, 103, 484, 515), "01 / HARMONICITY ORBIT")
        cx,cy = 254,320
        v = np.maximum(s,0)/max(float(s.max()),1e-12)
        for r in (70,110,150):
            d.ellipse((cx-r,cy-r,cx+r,cy+r),outline=(31,49,65))
        for i,value in enumerate(v):
            angle = 2*np.pi*i/len(v)-np.pi/2
            r = 77+82*value
            xy = (cx+r*np.cos(angle), cy+r*np.sin(angle))
            d.line((cx+72*np.cos(angle),cy+72*np.sin(angle),*xy),fill=teal,width=3)
            if i%10 == 0:
                label(cx+165*np.cos(angle)-14,cy+165*np.sin(angle)-8,f"{f[i]:g}")
        label(cx-46,cy-23,"MEAN H(f)")
        label(cx-49,cy+1,f"{mean:.3f}",white,self.big)
        label(44,479,"Clockwise: Hz  /  ray length: relative H(f)")

        card((500, 103, 1064, 682), "02 / POWER-WEIGHTED ACTIVATION")
        label(521,143,"A(i,j) = similarity(i,j) x p(i) x p(j)",gold)
        tile = Image.fromarray(activation_rgb(a,ceiling)).transpose(Image.Transpose.FLIP_TOP_BOTTOM).resize((430,430),Image.Resampling.NEAREST)
        im.paste(tile,(563,185))
        d.rectangle((562,184,994,616),outline=(90,111,126))
        for q in (0,.25,.5,.75,1):
            hz = f[0]+q*(f[-1]-f[0])
            label(554+430*q,619,f"{hz:g}")
            label(519,602-430*q,f"{hz:g}")
        label(1004,393,"Hz")
        bar = activation_rgb(np.linspace(0,1,220)[None,:]*ceiling,ceiling)
        im.paste(Image.fromarray(bar).resize((220,9)),(565,651))
        label(799,643,f"0 - {ceiling:.3g}  (log color)")
        label(1017,619,"Hz")

        card((1080, 103, 1576, 290), "03 / INPUT WINDOW")
        rms = float(np.sqrt(np.mean(wave**2)))
        plot(wave[::2],(1101,151,1555,248),teal,lo=-max(abs(wave).max(),1),hi=max(abs(wave).max(),1))
        label(1101,258,f"{len(wave)/waveform.meta['sfreq']:.1f} s at {waveform.meta['sfreq']:g} Hz    RMS {rms:.3f}")

        card((1080, 306, 1576, 503), "04 / POWER DISTRIBUTION")
        db = 10*np.log10(np.maximum(p,1e-8))
        plot(db,(1101,352,1555,453),gold,lo=-60,hi=0)
        label(1101,466,f"{f[0]:g} - {f[-1]:g} Hz    normalized PSD    -60 to 0 dB")

        card((1080, 519, 1576, 682), "05 / HARMONIC PEAKS + STRONGEST PAIR")
        label(1100,557,"  ".join(f"{val:g}" for val in pk if np.isfinite(val))+" Hz",white,self.body)
        off = a.copy()
        np.fill_diagonal(off,0)
        i,j = np.unravel_index(np.argmax(off),off.shape)
        label(1100,596,f"{f[i]:g} Hz  <-->  {f[j]:g} Hz",gold,self.body)
        label(1100,634,f"Pair activation {off[i,j]:.4g}  /  diagonal excluded here")

        card((24, 531, 484, 682), "06 / SPECTRAL COMPLEXITY")
        label(44,570,f"Entropy    {c[1]:.3f}",white,self.body)
        label(264,570,f"Flatness  {c[0]:.3f}",white,self.body)
        label(44,618,f"Spread     {c[2]:.2f} Hz",gold,self.body)
        label(264,618,f"Higuchi   {c[3]:.3f}",gold,self.body)

        card((24, 698, 1064, 963), "07 / H(f) HISTORY  -  time flows right")
        hist = np.stack([item[1] for item in self.history],axis=1)
        # Keep dark space to the left while history fills.
        canvas = np.zeros((len(s),180))
        canvas[:,-hist.shape[1]:] = hist
        heat = activation_rgb(canvas,max(float(canvas.max()),1e-9))
        im.paste(Image.fromarray(heat[::-1]).resize((967,170)),(77,744))
        label(38,741,f"{f[-1]:g}")
        label(38,895,f"{f[0]:g}")
        span = self.history[-1][0]-self.history[0][0]
        label(77,928,f"Hz    {span:.1f} seconds collected / 180 analysis frames / relative log color")

        card((1080, 698, 1576, 963), "08 / LIVE TRENDS")
        tr = np.array(self.trends)
        for idx,color,title,y in ((1,teal,"Mean H",744),(2,gold,"Entropy",808)):
            vals = tr[:,idx]
            plot(vals,(1182,y,1555,y+42),color)
            label(1100,y+8,title,color)
        label(1100,886,"Each trace auto-ranges over visible history.")
        label(1100,919,f"Channel: {channel}",muted)
        card((24, 980, 1064, 1187), "09 / HARMONIC SPECTRUM H(f)  -  why these metrics move")
        x0,y0,x1,y1 = 77,1030,1042,1125
        weights = np.maximum(s,0)/max(float(np.maximum(s,0).sum()),1e-12)
        centroid = float(np.sum(f*weights))
        def fx(hz):
            return x0+(np.clip(hz,f[0],f[-1])-f[0])/(f[-1]-f[0])*(x1-x0)
        spread = float(c[2]) if np.isfinite(c[2]) else 0
        d.rectangle((fx(centroid-spread),y0,fx(centroid+spread),y1),fill=(29,48,65))
        maximum = max(float(s.max()),1e-12)
        plot(s,(x0,y0,x1,y1),teal,lo=0,hi=maximum)
        arithmetic = float(np.mean(s))
        geometric = float(np.exp(np.mean(np.log(np.maximum(s,0)+np.finfo(float).eps))))
        for value,color in ((arithmetic,gold),(geometric,(175,141,250))):
            yy = y1-(y1-y0)*np.clip(value/maximum,0,1)
            for xx in range(x0,x1,14):
                d.line((xx,yy,min(xx+7,x1),yy),fill=color,width=2)
        d.line((fx(centroid),y0,fx(centroid),y1),fill=white)
        for val in pk[np.isfinite(pk)]:
            idx = np.argmin(abs(f-val))
            xx,yy = fx(val),y1-s[idx]/maximum*(y1-y0)
            d.ellipse((xx-4,yy-4,xx+4,yy+4),fill=gold)
        label(37,1026,f"{maximum:.1f}")
        label(50,1110,"0")
        for hz in np.linspace(f[0],f[-1],8):
            label(fx(hz)-10,1128,f"{hz:g}")
        label(78,1159,"Hz   |   band: centroid +/- spread",muted)
        label(431,1159,"-- arithmetic mean",gold)
        label(641,1159,"-- geometric mean",(175,141,250))
        label(859,1159,"dots: H peaks",gold)

        card((1080, 980, 1576, 1187), "10 / DISTRIBUTION INTUITION  -  H(f)")
        entropy_norm = np.clip(float(c[1])/np.log2(len(s)),0,1) if np.isfinite(c[1]) else 0
        label(1100,1018,f"Entropy {c[1]:.2f} bits / {np.log2(len(s)):.2f} max")
        d.rounded_rectangle((1100,1047,1553,1057),radius=4,fill=(33,47,64))
        d.rounded_rectangle((1100,1047,1100+max(4,453*entropy_norm),1057),radius=4,fill=teal)
        label(1100,1065,"concentrated",muted)
        label(1440,1065,"distributed",muted)
        flatness = np.clip(float(c[0]),0,1) if np.isfinite(c[0]) else 0
        label(1100,1093,f"Flatness {c[0]:.3f} = geometric / arithmetic")
        d.rounded_rectangle((1100,1122,1553,1132),radius=4,fill=(33,47,64))
        d.rounded_rectangle((1100,1122,1100+max(4,453*flatness),1132),radius=4,fill=(175,141,250))
        label(1100,1143,"peaky",muted)
        label(1450,1143,"level / even",muted)

        # Travelling pulses explain analysis direction, not literal packet latency.
        def flow(points, color, strength=1):
            d.line(points,fill=tuple(int(v*.35) for v in color),width=2)
            pts = np.asarray(points,float)
            lengths = np.linalg.norm(np.diff(pts,axis=0),axis=1)
            total = lengths.sum()
            for phase in (0,.33,.66):
                dist = ((now*.28+phase)%1)*total
                for k,length in enumerate(lengths):
                    if dist <= length:
                        xy = pts[k]+(pts[k+1]-pts[k])*dist/max(length,1e-9)
                        radius = 2+2*np.clip(strength,0,1)
                        d.ellipse((xy[0]-radius,xy[1]-radius,xy[0]+radius,xy[1]+radius),fill=color)
                        break
                    dist -= length
            end,prev = pts[-1],pts[-2]
            delta = (end-prev)/max(np.linalg.norm(end-prev),1e-9)
            perp = np.array([-delta[1],delta[0]])
            d.polygon([tuple(end),tuple(end-delta*8+perp*4),tuple(end-delta*8-perp*4)],fill=color)
        flow([(1515,289),(1515,306)],teal,min(rms,1))
        flow([(1080,406),(1064,406)],gold,float(p.max())*5)
        flow([(780,682),(780,690),(1072,690),(1072,972),(540,972),(540,980)],gold,float(a.max())/ceiling)
        flow([(300,980),(300,963)],teal)
        flow([(500,322),(484,322)],teal)
        label(28,1195,"Flow: input > power > activation > H(f) > history  |  moving dots show processing direction  |  matrix uses normalized power")
        # Keep the image compact for the live transport/viewer feeds.
        im = im.resize((825, 629), Image.Resampling.BILINEAR)
        frame = (np.asarray(im).astype(np.float32)/255,image_meta)
        return {"dashboard": frame}
