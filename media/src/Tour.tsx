import React from "react";
import {
  AbsoluteFill,
  Easing,
  Img,
  Sequence,
  interpolate,
  spring,
  staticFile,
  useCurrentFrame,
  useVideoConfig,
} from "remotion";

const cream = "#f7f3ec";
const creamHigh = "#fffaf3";
const ink = "#26221d";
const secondary = "#6b6258";
const tertiary = "#72685c";
const accent = "#aa5431";
const surface = "#fffcf6";
const surfaceMuted = "#f2ebe1";
const line = "#e8dfd3";
const track = "#ece4d8";
const bar = "#95877a";
const barHover = "#897a67";

const font =
  '"Ubuntu Sans", Ubuntu, Cantarell, "DejaVu Sans", "Noto Sans", sans-serif';
const mono = '"Ubuntu Mono", "DejaVu Sans Mono", monospace';

const cl = {
  extrapolateLeft: "clamp" as const,
  extrapolateRight: "clamp" as const,
};
const easeOut = Easing.bezier(0.16, 1, 0.3, 1);
const easeInOut = Easing.bezier(0.65, 0, 0.35, 1);

const ramp = (
  f: number,
  start: number,
  end: number,
  from = 0,
  to = 1,
  easing = easeOut,
) =>
  interpolate(f, [start, end], [from, to], {
    ...cl,
    easing,
  });

const fadeOut = (f: number, start: number, end: number) =>
  interpolate(f, [start, end], [1, 0], { ...cl, easing: easeInOut });

const DriftOrb: React.FC<{
  f: number;
  x: number;
  y: number;
  size?: number;
  opacity?: number;
}> = ({ f, x, y, size = 720, opacity = 0.05 }) => (
  <div
    style={{
      position: "absolute",
      left: x + Math.sin(f / 110) * 44,
      top: y + Math.cos(f / 140) * 34,
      width: size,
      height: size,
      borderRadius: "50%",
      background: `rgba(170,84,49,${opacity})`,
      filter: "blur(90px)",
      pointerEvents: "none",
    }}
  />
);

/* ------------------------------------------------------------------ data */

const WEEK = [
  { label: "Sun 18 Oct", ratio: 0.63 },
  { label: "Mon 19 Oct", ratio: 0.42 },
  { label: "Tue 20 Oct", ratio: 0.56 },
  { label: "Wed 21 Oct", ratio: 0.25 },
  { label: "Thu 22 Oct", ratio: 0.59 },
  { label: "Fri 23 Oct", ratio: 0.46 },
  { label: "Sat 24 Oct", ratio: 0.67, today: true },
];

const APPS = [
  { name: "Brave Web Browser", icon: "apps/brave.png", duration: "2h 54m", ratio: 1, tint: "#8c5a3c" },
  { name: "Terminal", icon: "apps/terminal.png", duration: "1h 20m", ratio: 0.46, tint: "#5f7a5c" },
  { name: "Files", icon: "apps/files.png", duration: "50m", ratio: 0.29, tint: "#5c6f8a" },
  { name: "Image Viewer", icon: "apps/image-viewer.png", duration: "35m", ratio: 0.2, tint: "#80607a" },
  { name: "Text Editor", icon: "apps/text-editor.png", duration: "28m", ratio: 0.16, tint: "#9a6a2f" },
  { name: "VLC media player", icon: "apps/vlc.png", duration: "17m", ratio: 0.1, tint: "#4f7b78" },
  { name: "Settings", icon: "apps/settings.png", duration: "13m", ratio: 0.075, tint: "#626a72" },
];

const HOURS = [
  0, 0, 0, 0, 0, 0, 0.08, 0.42, 0.92, 0.84, 0.78, 0.88, 0.92, 0.86,
  0.74, 0.66, 0.3, 0, 0, 0, 0, 0, 0, 0,
];

const HISTORY = [
  { day: "Today", date: "24 Oct", time: "6h 44m", ratio: 0.86, today: true },
  { day: "Yesterday", date: "23 Oct", time: "5h 57m", ratio: 0.77 },
  { day: "Thursday", date: "22 Oct", time: "7h 51m", ratio: 1 },
  { day: "Wednesday", date: "21 Oct", time: "3h 29m", ratio: 0.44 },
  { day: "Tuesday", date: "20 Oct", time: "6h 27m", ratio: 0.82 },
  { day: "Monday", date: "19 Oct", time: "5h 29m", ratio: 0.7 },
  { day: "Sunday", date: "18 Oct", time: "7h 26m", ratio: 0.95 },
  { day: "Saturday", date: "17 Oct", time: "4h 30m", ratio: 0.57 },
];

/* --------------------------------------------------------------- pieces */

const BrandMark: React.FC<{ size?: number }> = ({ size = 1 }) => (
  <div
    style={{
      width: 20 * size,
      height: 15 * size,
      position: "relative",
      flex: "none",
    }}
  >
    <div
      style={{
        position: "absolute",
        left: 2 * size,
        top: 0,
        width: 16 * size,
        height: 8 * size,
        overflow: "hidden",
      }}
    >
      <div
        style={{
          width: 16 * size,
          height: 16 * size,
          borderRadius: 8 * size,
          background: accent,
        }}
      />
    </div>
    <div
      style={{
        position: "absolute",
        left: 0,
        top: 10 * size,
        width: 20 * size,
        height: 2 * size,
        borderRadius: 1 * size,
        background: accent,
      }}
    />
    <div
      style={{
        position: "absolute",
        left: 0,
        top: 14 * size,
        width: 12 * size,
        height: 1 * size,
        background: accent,
        opacity: 0.45,
      }}
    />
  </div>
);

const Sliders: React.FC = () => (
  <svg width="16" height="16" viewBox="0 0 16 16" fill="none">
    <path d="M1 4h14M1 8h14M1 12h14" stroke={secondary} strokeWidth="1.4" />
    <circle cx="5" cy="4" r="2" fill={cream} stroke={secondary} strokeWidth="1.4" />
    <circle cx="11" cy="8" r="2" fill={cream} stroke={secondary} strokeWidth="1.4" />
    <circle cx="6" cy="12" r="2" fill={cream} stroke={secondary} strokeWidth="1.4" />
  </svg>
);

const WinControl: React.FC<{ kind: "min" | "max" | "close"; hover?: boolean }> = ({
  kind,
  hover,
}) => (
  <div
    style={{
      width: 28,
      height: 28,
      borderRadius: 7,
      background: hover ? surfaceMuted : "transparent",
      display: "grid",
      placeItems: "center",
      position: "relative",
    }}
  >
    {kind === "min" ? (
      <div style={{ width: 10, height: 1.6, borderRadius: 1, background: secondary }} />
    ) : null}
    {kind === "max" ? (
      <div
        style={{
          width: 10,
          height: 10,
          border: `1.4px solid ${secondary}`,
          borderRadius: 2,
          boxSizing: "border-box",
        }}
      />
    ) : null}
    {kind === "close" ? (
      <div
        style={{
          width: 10,
          height: 10,
          position: "relative",
          color: secondary,
          fontSize: 23,
          lineHeight: "10px",
          textAlign: "center",
          fontWeight: 400,
          transform: "translateY(-1px)",
        }}
      >
        ×
      </div>
    ) : null}
  </div>
);

const Chrome: React.FC = () => (
  <div
    style={{
      height: 56,
      display: "flex",
      alignItems: "center",
      padding: "0 14px 0 40px",
      borderBottom: `1px solid ${line}`,
      gap: 12,
    }}
  >
    <div style={{ display: "flex", alignItems: "center", gap: 10 }}>
      <BrandMark />
      <div style={{ fontSize: 16, fontWeight: 600, letterSpacing: "0.1px" }}>
        Daymark
      </div>
    </div>
    <div style={{ flex: 1 }} />
    <div
      style={{
        width: 136,
        height: 32,
        borderRadius: 9,
        background: surfaceMuted,
        border: `1px solid ${line}`,
        display: "flex",
        padding: 3,
        gap: 2,
      }}
    >
      <div
        style={{
          flex: 1,
          borderRadius: 6,
          background: surface,
          display: "grid",
          placeItems: "center",
          fontSize: 12,
          fontWeight: 600,
        }}
      >
        Day
      </div>
      <div
        style={{
          flex: 1,
          display: "grid",
          placeItems: "center",
          fontSize: 12,
          fontWeight: 500,
          color: secondary,
        }}
      >
        History
      </div>
    </div>
    <div
      style={{
        width: 28,
        height: 28,
        borderRadius: 7,
        display: "grid",
        placeItems: "center",
      }}
    >
      <Sliders />
    </div>
    <div style={{ width: 1, height: 14, background: line }} />
    <div style={{ display: "flex", gap: 1 }}>
      <WinControl kind="min" />
      <WinControl kind="max" />
      <WinControl kind="close" hover />
    </div>
  </div>
);

const SectionHeader: React.FC<{
  title: string;
  caption: string;
  right?: React.ReactNode;
}> = ({ title, caption, right }) => (
  <div style={{ display: "flex", alignItems: "center", height: 24 }}>
    <div style={{ fontSize: 13, fontWeight: 600 }}>{title}</div>
    <div
      style={{
        marginLeft: 8,
        fontSize: 12,
        color: tertiary,
        whiteSpace: "nowrap",
      }}
    >
      {caption}
    </div>
    <div style={{ flex: 1 }} />
    {right}
  </div>
);

/* --------------------------------------------------------------- scenes */

const Hook: React.FC = () => {
  const f = useCurrentFrame();
  const out = fadeOut(f, 96, 140);
  const line1 = ramp(f, 8, 46, 0, 1);
  const line2 = ramp(f, 22, 62, 0, 1);
  const y1 = ramp(f, 8, 46, 34, 0);
  const y2 = ramp(f, 22, 62, 34, 0);
  const pill = ramp(f, 52, 86, 0, 1);
  const mark = ramp(f, 0, 26, 0.8, 1);
  const markOpacity = ramp(f, 0, 20);

  return (
    <AbsoluteFill
      style={{
        background: cream,
        fontFamily: font,
        color: ink,
        opacity: out,
      }}
    >
      <div
        style={{
          position: "absolute",
          inset: 0,
          background:
            `radial-gradient(1100px 800px at 18% -10%, ${creamHigh} 0%, rgba(255,250,243,0) 60%),` +
            "radial-gradient(900px 700px at 100% 110%, #eee3d5 0%, rgba(238,227,213,0) 55%)",
        }}
      />
      <DriftOrb f={f} x={-120} y={-200} size={900} opacity={0.055} />
      <div
        style={{
          position: "absolute",
          left: 144,
          top: 340,
          maxWidth: 1300,
        }}
      >
        <div
          style={{
            display: "flex",
            alignItems: "center",
            gap: 14,
            marginBottom: 44,
            opacity: markOpacity,
            transform: `scale(${mark})`,
            transformOrigin: "left center",
          }}
        >
          <BrandMark size={1.35} />
          <div style={{ fontSize: 20, fontWeight: 600, letterSpacing: "0.01em" }}>
            Daymark
          </div>
        </div>
        <div style={{ overflow: "hidden" }}>
          <div
            style={{
              fontSize: 92,
              lineHeight: 1.02,
              fontWeight: 600,
              letterSpacing: "-0.045em",
              opacity: line1,
              transform: `translateY(${y1}px)`,
            }}
          >
            Your screen time.
          </div>
        </div>
        <div style={{ overflow: "hidden", paddingBottom: 8 }}>
          <div
            style={{
              fontSize: 92,
              lineHeight: 1.02,
              fontWeight: 600,
              letterSpacing: "-0.045em",
              color: accent,
              opacity: line2,
              transform: `translateY(${y2}px)`,
            }}
          >
            Without the cloud.
          </div>
        </div>
        <div
          style={{
            display: "flex",
            alignItems: "center",
            gap: 14,
            marginTop: 38,
            opacity: pill,
          }}
        >
          <div style={{ width: 42, height: 3, borderRadius: 2, background: accent }} />
          <div style={{ fontSize: 25, color: secondary }}>
            100% local · no account · no telemetry
          </div>
        </div>
      </div>
    </AbsoluteFill>
  );
};

/* ----------------------------------------------------------- dashboard */

const Hero: React.FC<{ f: number }> = ({ f }) => {
  const counted = interpolate(f, [55, 210], [0, 24240], {
    ...cl,
    easing: Easing.bezier(0.24, 1, 0.32, 1),
  });
  const seconds = f < 210 ? counted : 24240 + Math.floor((f - 210) / 60) * 60;
  const h = Math.floor(seconds / 3600);
  const m = Math.floor((seconds % 3600) / 60);
  const total = h > 0 ? `${h}h ${m}m` : `${m}m`;
  const show = f > 60;

  return (
    <div style={{ marginTop: 26 }}>
      <div
        style={{
          fontSize: 11,
          fontWeight: 600,
          letterSpacing: "1.1px",
          color: secondary,
          opacity: ramp(f, 20, 45),
        }}
      >
        TODAY · SATURDAY, 24 OCTOBER
      </div>
      <div
        style={{
          height: 76,
          display: "flex",
          alignItems: "center",
          fontSize: 62,
          fontWeight: 600,
          letterSpacing: "-1.5px",
          color: ink,
          opacity: ramp(f, 25, 55),
          fontVariantNumeric: "tabular-nums",
        }}
      >
        {show ? total : "0m"}
      </div>
      <div
        style={{
          fontSize: 13,
          color: secondary,
          opacity: ramp(f, 80, 110),
        }}
      >
        47m more than yesterday.
      </div>
    </div>
  );
};

const WeekChart: React.FC<{ f: number }> = ({ f }) => (
  <div style={{ marginTop: 30 }}>
    <div
      style={{
        display: "flex",
        alignItems: "center",
        fontSize: 13,
        fontWeight: 600,
        opacity: ramp(f, 70, 100),
      }}
    >
      Last 7 days
      <div style={{ flex: 1 }} />
      <div style={{ fontSize: 12, fontWeight: 400, color: tertiary }}>43h 23m</div>
    </div>
    <div style={{ display: "flex", gap: 6, marginTop: 14 }}>
      {WEEK.map((d, i) => {
        const p = spring({
          frame: f - (90 + i * 5),
          fps: 60,
          config: { damping: 20, stiffness: 120, mass: 0.9 },
        });
        return (
          <div
            key={d.label}
            style={{
              flex: 1,
              display: "flex",
              flexDirection: "column",
              alignItems: "center",
            }}
          >
            <div style={{ height: 104, width: "100%", position: "relative" }}>
              <div
                style={{
                  position: "absolute",
                  left: "50%",
                  bottom: 0,
                  width: 26,
                  height: Math.max(4, 104 * d.ratio * Math.min(1, p)),
                  marginLeft: -13,
                  borderRadius: 5,
                  background: d.today ? accent : bar,
                }}
              />
            </div>
            <div
              style={{
                height: 15,
                marginTop: 8,
                fontSize: 12,
                fontWeight: d.today ? 600 : 500,
                color: d.today ? accent : tertiary,
                whiteSpace: "nowrap",
              }}
            >
              {d.label}
            </div>
            <div
              style={{
                width: 3,
                height: 3,
                borderRadius: 2,
                marginTop: 2,
                background: d.today ? accent : "transparent",
              }}
            />
          </div>
        );
      })}
    </div>
  </div>
);

const AppRow: React.FC<{
  app: (typeof APPS)[number];
  index: number;
  f: number;
  progress: number;
  query: string;
}> = ({ app, index, f, progress, query }) => {
  const enter = spring({
    frame: f - (140 + index * 6),
    fps: 60,
    config: { damping: 22, stiffness: 130, mass: 0.8 },
  });
  const q = query.toLowerCase();
  const matches = q.length === 0 || app.name.toLowerCase().includes(q);
  const dim = q.length > 0 && !matches;
  const matchesList = APPS.filter((a) => a.name.toLowerCase().includes(q));
  const target = q.length === 0 ? index : matchesList.findIndex((a) => a.name === app.name);
  const pos = index + (matches ? target - index : 0) * progress;
  const y = (pos - index) * 48;

  return (
    <div
      style={{
        position: "absolute",
        left: 0,
        right: 0,
        top: index * 48 + y,
        height: 48,
        display: "flex",
        alignItems: "center",
        padding: "0 8px 0 6px",
        gap: 10,
        opacity: Math.min(1, enter) * (dim ? 1 - progress : 1),
        transform: `translateX(${(1 - Math.min(1, enter)) * 18}px) scaleY(${
          dim ? 1 - progress * 0.15 : 1
        })`,
      }}
    >
      <Img
        src={staticFile(app.icon)}
        style={{ width: 28, height: 28, borderRadius: 7, flex: "none" }}
      />
      <div
        style={{
          flex: 1,
          minWidth: 0,
          display: "flex",
          flexDirection: "column",
          gap: 5,
        }}
      >
        <div
          style={{
            fontSize: 13,
            fontWeight: 500,
            whiteSpace: "nowrap",
            overflow: "hidden",
            textOverflow: "ellipsis",
          }}
        >
          {app.name}
        </div>
        <div style={{ height: 3, borderRadius: 2, background: track }}>
          <div
            style={{
              width: `${app.ratio * 100}%`,
              height: "100%",
              borderRadius: 2,
              background: app.tint,
              opacity: 0.55,
              transform: `scaleX(${Math.min(1, enter)})`,
              transformOrigin: "left center",
            }}
          />
        </div>
      </div>
      <div
        style={{
          width: 58,
          textAlign: "right",
          fontSize: 12,
          fontWeight: 500,
          color: secondary,
        }}
      >
        {app.duration}
      </div>
    </div>
  );
};

const SearchField: React.FC<{ f: number; query: string; focused: boolean }> = ({
  f,
  query,
  focused,
}) => {
  const caret = focused && Math.floor(f / 24) % 2 === 0;
  return (
    <div
      style={{
        width: 184,
        height: 30,
        borderRadius: 8,
        background: surface,
        border: `1px solid ${focused ? accent : line}`,
        display: "flex",
        alignItems: "center",
        padding: "0 10px",
        gap: 8,
        marginLeft: "auto",
      }}
    >
      <svg width="14" height="14" viewBox="0 0 16 16" fill="none">
        <circle cx="7" cy="7" r="4.4" stroke={focused ? secondary : tertiary} strokeWidth="1.5" />
        <path d="M10.4 10.4L13.6 13.6" stroke={focused ? secondary : tertiary} strokeWidth="1.5" strokeLinecap="round" />
      </svg>
      <div
        style={{
          fontSize: 13,
          color: query ? ink : tertiary,
          whiteSpace: "nowrap",
          display: "flex",
          alignItems: "center",
        }}
      >
        {query ? query : "Find an app"}
        {caret ? (
          <div
            style={{
              width: 1.5,
              height: 16,
              background: accent,
              marginLeft: 2,
            }}
          />
        ) : null}
      </div>
      {query ? (
        <div
          style={{
            marginLeft: "auto",
            width: 20,
            height: 20,
            borderRadius: 10,
            display: "grid",
            placeItems: "center",
            fontSize: 15,
            color: tertiary,
            background: surfaceMuted,
          }}
        >
          ×
        </div>
      ) : null}
    </div>
  );
};

const AppList: React.FC<{
  f: number;
  query: string;
  focused: boolean;
  progress: number;
  caption: string;
}> = ({ f, query, focused, progress, caption }) => (
  <div style={{ flex: 1, minWidth: 0 }}>
    <SectionHeader
      title="Applications"
      caption={
        query
          ? `${APPS.filter((a) => a.name.toLowerCase().includes(query.toLowerCase())).length} of 7 apps`
          : "7 apps"
      }
      right={<SearchField f={f} query={query} focused={focused} />}
    />
    <div style={{ position: "relative", height: 7 * 48, marginTop: 6 }}>
      {APPS.map((app, i) => (
        <AppRow
          key={app.name}
          app={app}
          index={i}
          f={f}
          progress={progress}
          query={query}
        />
      ))}
    </div>
    <div
      style={{
        marginTop: 8,
        fontSize: 12,
        color: tertiary,
        opacity: ramp(f, 260, 300),
      }}
    >
      {caption}
    </div>
  </div>
);

const HourChart: React.FC<{ f: number }> = ({ f }) => (
  <div style={{ width: 364, flex: "none", display: "flex", flexDirection: "column" }}>
    <div style={{ display: "flex", alignItems: "center", height: 18 }}>
      <div style={{ fontSize: 13, fontWeight: 600 }}>By hour</div>
      <div style={{ flex: 1 }} />
      <div style={{ fontSize: 11, color: tertiary, whiteSpace: "nowrap" }}>
        Peak 9–10 AM
      </div>
    </div>
    <div style={{ height: 20 }} />
    <div style={{ height: 116, display: "flex", gap: 3, alignItems: "flex-end" }}>
      {HOURS.map((v, i) => {
        const p = spring({
          frame: f - (110 + i * 4),
          fps: 60,
          config: { damping: 20, stiffness: 120, mass: 0.9 },
        });
        return (
          <div
            key={i}
            style={{
              flex: 1,
              height: "100%",
              display: "flex",
              alignItems: "flex-end",
            }}
          >
            {v > 0 ? (
              <div
                style={{
                  width: "100%",
                  height: `${v * 100}%`,
                  borderRadius: 3,
                  background: accent,
                  opacity: 0.78,
                  transform: `scaleY(${Math.min(1, p)})`,
                  transformOrigin: "bottom center",
                }}
              />
            ) : null}
          </div>
        );
      })}
    </div>
    <div style={{ height: 1, background: line, marginTop: 0 }} />
    <div style={{ height: 8 }} />
    <div style={{ position: "relative", height: 14 }}>
      {[
        { t: "12 AM", x: 0, w: 44, align: "left" as const },
        { t: "6 AM", x: 0.25, align: "center" as const },
        { t: "12 PM", x: 0.5, align: "center" as const },
        { t: "6 PM", x: 0.75, align: "center" as const },
        { t: "12 AM", x: 1, w: 44, align: "right" as const },
      ].map((l, i) => (
        <div
          key={i}
          style={{
            position: "absolute",
            top: 0,
            width: l.w ?? 40,
            left: l.align === "left" ? 0 : l.align === "right" ? "100%" : `${l.x * 100}%`,
            transform:
              l.align === "right"
                ? "translateX(-100%)"
                : l.align === "center"
                  ? "translateX(-50%)"
                  : undefined,
            fontSize: 11,
            color: tertiary,
            textAlign: l.align,
            whiteSpace: "nowrap",
          }}
        >
          {l.t}
        </div>
      ))}
    </div>
  </div>
);

const DashboardUI: React.FC<{
  f: number;
  query: string;
  focused: boolean;
  progress: number;
}> = ({ f, query, focused, progress }) => (
  <div
    style={{
      width: 1120,
      height: 780,
      background: cream,
      borderRadius: 20,
      overflow: "hidden",
      border: "1px solid rgba(38,34,29,0.10)",
      fontFamily: font,
      color: ink,
    }}
  >
    <Chrome />
    <div style={{ padding: "24px 40px 0" }}>
      <Hero f={f} />
      <WeekChart f={f} />
      <div style={{ display: "flex", gap: 24, marginTop: 26 }}>
        <div style={{ width: 616, display: "flex" }}>
          <AppList
            f={f}
            query={query}
            focused={focused}
            progress={progress}
            caption="Every app, ranked by the time it actually received."
          />
        </div>
        <div style={{ width: 1, background: line }} />
        <HourChart f={f} />
      </div>
    </div>
  </div>
);

/* --------------------------------------------------------------- cursor */

const Cursor: React.FC<{
  f: number;
  scale: number;
  x: number;
  y: number;
  click: number;
}> = ({ f, scale, x, y, click }) => {
  const s = 1 / scale;
  const press = interpolate(f, [click, click + 5, click + 10], [1, 0.86, 1], cl);
  const ripple = interpolate(f, [click, click + 24], [0, 1], cl);
  return (
    <div
      style={{
        position: "absolute",
        left: x,
        top: y,
        width: 0,
        height: 0,
        transform: `scale(${s})`,
        transformOrigin: "0 0",
        pointerEvents: "none",
        zIndex: 40,
      }}
    >
      <div
        style={{
          position: "absolute",
          left: -10,
          top: -8,
          opacity: ripple > 0 && ripple < 1 ? 1 - ripple : 0,
          width: 46 * ripple,
          height: 46 * ripple,
          marginLeft: -23 * ripple,
          marginTop: -23 * ripple,
          borderRadius: "50%",
          border: `2px solid ${accent}`,
        }}
      />
      <svg
        width="26"
        height="30"
        viewBox="0 0 26 30"
        fill="none"
        style={{
          transform: `scale(${press})`,
          transformOrigin: "2px 2px",
          filter: "drop-shadow(0 4px 8px rgba(38,34,29,0.28))",
        }}
      >
        <path
          d="M2 2 L2 23 L8.5 17.5 L13 28 L17.5 26 L13 16 L22 16 Z"
          fill="#fffdf9"
          stroke="#26221d"
          strokeWidth="1.6"
          strokeLinejoin="round"
        />
      </svg>
    </div>
  );
};

/* ------------------------------------------------------------ dashboard */

type Cam = { s: number; x: number; y: number };

const CAM_KEYS: { f: number; cam: Cam }[] = [
  { f: 0, cam: { s: 0.995, x: 372, y: 320 } },
  { f: 55, cam: { s: 1.05, x: 372, y: 172 } },
  { f: 270, cam: { s: 1.1, x: 340, y: 128 } },
  { f: 380, cam: { s: 2.02, x: 40, y: -400 } },
  { f: 600, cam: { s: 2.08, x: 18, y: -430 } },
  { f: 720, cam: { s: 1.1, x: 340, y: 128 } },
];

const camAt = (f: number): Cam => {
  for (let i = 0; i < CAM_KEYS.length - 1; i++) {
    const a = CAM_KEYS[i];
    const b = CAM_KEYS[i + 1];
    if (f <= a.f) return a.cam;
    if (f <= b.f) {
      const p = easeInOut((f - a.f) / (b.f - a.f));
      return {
        s: a.cam.s + (b.cam.s - a.cam.s) * p,
        x: a.cam.x + (b.cam.x - a.cam.x) * p,
        y: a.cam.y + (b.cam.y - a.cam.y) * p,
      };
    }
  }
  return CAM_KEYS[CAM_KEYS.length - 1].cam;
};

const Chapter: React.FC<{
  f: number;
  from: number;
  to: number;
  text: string;
}> = ({ f, from, to, text }) => {
  const inP = ramp(f, from, from + 22);
  const outP = fadeOut(f, to - 18, to);
  const p = Math.min(inP, outP);
  if (p <= 0) return null;
  return (
    <div
      style={{
        position: "absolute",
        left: 96,
        bottom: 26,
        display: "flex",
        alignItems: "center",
        gap: 16,
        padding: "14px 24px",
        borderRadius: 14,
        background: "rgba(247,243,236,0.94)",
        border: "1px solid rgba(38,34,29,0.08)",
        boxShadow: "0 18px 50px rgba(38,34,29,0.10)",
        opacity: p,
        transform: `translateY(${(1 - inP) * 16}px)`,
        fontSize: 30,
        fontWeight: 600,
        letterSpacing: "-0.025em",
        zIndex: 30,
      }}
    >
      <div style={{ width: 26, height: 3, borderRadius: 2, background: accent }} />
      {text}
    </div>
  );
};

const DashboardScene: React.FC = () => {
  const f = useCurrentFrame();
  const { fps } = useVideoConfig();
  const cam = camAt(f);
  const enter = spring({
    frame: f,
    fps,
    config: { damping: 24, stiffness: 90, mass: 1.1 },
  });
  const enterOpacity = ramp(f, 0, 26);
  const windowScale = cam.s * (0.99 + 0.01 * Math.min(1, enter));

  // search interaction
  const query = f < 450 ? "" : f < 466 ? "t" : f < 540 ? "te" : "";
  const focused = f >= 428 && f < 540;
  const filterStart = 452;
  const clearStart = 540;
  const progress = Math.min(
    ramp(f, filterStart, filterStart + 26),
    f >= clearStart ? 1 - ramp(f, clearStart, clearStart + 26) : 1,
  );

  // cursor path (UI coordinates)
  const cursorKeys = [
    { f: 400, x: 1080, y: 900 },
    { f: 430, x: 560, y: 441 },
    { f: 500, x: 620, y: 441 },
    { f: 540, x: 661, y: 441 },
    { f: 606, x: 760, y: 640 },
    { f: 720, x: 760, y: 640 },
  ];
  const ck = (() => {
    for (let i = 0; i < cursorKeys.length - 1; i++) {
      const a = cursorKeys[i];
      const b = cursorKeys[i + 1];
      if (f <= a.f) return { x: a.x, y: a.y };
      if (f <= b.f) {
        const p = easeInOut((f - a.f) / (b.f - a.f));
        return { x: a.x + (b.x - a.x) * p, y: a.y + (b.y - a.y) * p };
      }
    }
    return cursorKeys[cursorKeys.length - 1];
  })();
  const cursorVisible = f > 396 && f < 606;

  return (
    <AbsoluteFill style={{ background: cream, fontFamily: font, overflow: "hidden" }}>
      <div
        style={{
          position: "absolute",
          inset: 0,
          background:
            `radial-gradient(900px 700px at 12% -8%, ${creamHigh} 0%, rgba(255,250,243,0) 58%),` +
            "radial-gradient(800px 700px at 105% 108%, #eee3d3 0%, rgba(238,227,211,0) 55%)",
        }}
      />
      <DriftOrb f={f} x={-160} y={-220} size={900} opacity={0.055} />
      <div
        style={{
          position: "absolute",
          left: 0,
          top: 0,
          width: 1120,
          height: 780,
          transform: `translate(${cam.x}px, ${cam.y + (1 - enter) * 90}px) scale(${windowScale})`,
          transformOrigin: "0 0",
          opacity: enterOpacity,
          boxShadow: "0 60px 160px rgba(38,34,29,0.22), 0 12px 40px rgba(38,34,29,0.10)",
          borderRadius: 20,
        }}
      >
        <DashboardUI f={f} query={query} focused={focused} progress={progress} />
        {cursorVisible ? (
          <Cursor
            f={f}
            scale={windowScale}
            x={ck.x}
            y={ck.y}
            click={f >= 528 ? 540 : 430}
          />
        ) : null}
      </div>

      <Chapter f={f} from={55} to={280} text="One day. One screen." />
      <Chapter f={f} from={290} to={600} text="Every app, accounted for." />
      <Chapter f={f} from={610} to={718} text="And the shape of the day." />
    </AbsoluteFill>
  );
};

/* --------------------------------------------------------------- privacy */

const Pill: React.FC<{ label: string; p: number; y: number }> = ({ label, p, y }) => (
  <div
    style={{
      display: "flex",
      alignItems: "center",
      gap: 12,
      height: 60,
      padding: "0 26px",
      borderRadius: 999,
      border: `1px solid ${line}`,
      background: "rgba(255,252,246,0.8)",
      color: secondary,
      fontSize: 22,
      opacity: p,
      transform: `translateY(${y}px)`,
    }}
  >
    <div style={{ width: 7, height: 7, borderRadius: 4, background: accent }} />
    {label}
  </div>
);

const Privacy: React.FC = () => {
  const f = useCurrentFrame();
  const line1 = ramp(f, 4, 34);
  const line2 = ramp(f, 12, 44);
  const y1 = ramp(f, 4, 34, 30, 0);
  const y2 = ramp(f, 12, 44, 30, 0);
  const rule = ramp(f, 26, 54);
  const chips = [
    { label: "No account", at: 36 },
    { label: "No cloud", at: 44 },
    { label: "No telemetry", at: 52 },
  ];

  const push = interpolate(f, [0, 180], [1, 1.05], cl);
  const db = ramp(f, 84, 112);

  return (
    <AbsoluteFill style={{ background: cream, fontFamily: font, overflow: "hidden" }}>
      <div
        style={{
          position: "absolute",
          inset: 0,
          background:
            `radial-gradient(1000px 760px at 50% 46%, rgba(170,84,49,0.09) 0%, rgba(170,84,49,0) 66%),` +
            `radial-gradient(1200px 800px at 10% -10%, ${creamHigh} 0%, rgba(255,250,243,0) 60%)`,
        }}
      />
      <DriftOrb f={f} x={-120} y={-160} size={900} opacity={0.1} />
      <div
        style={{
          position: "absolute",
          left: "50%",
          top: "46%",
          width: 620,
          height: 620,
          marginLeft: -310,
          marginTop: -310,
          borderRadius: "50%",
          background: "rgba(170,84,49,0.05)",
          filter: "blur(70px)",
          transform: `translateY(${Math.sin(f / 60) * 14}px) scale(${push})`,
        }}
      />
      <div
        style={{
          position: "absolute",
          left: 0,
          right: 0,
          top: 292,
          display: "flex",
          flexDirection: "column",
          alignItems: "center",
          transform: `scale(${push}) translate(${6 * (f / 180)}px, ${
            -34 * (f / 180)
          }px)`,
          transformOrigin: "50% 40%",
        }}
      >
        <div style={{ overflow: "hidden" }}>
          <div
            style={{
              fontSize: 78,
              lineHeight: 1.08,
              fontWeight: 600,
              letterSpacing: "-0.04em",
              opacity: line1,
              transform: `translateY(${y1}px)`,
              color: ink,
            }}
          >
            Your day stays
          </div>
        </div>
        <div style={{ overflow: "hidden", paddingBottom: 6 }}>
          <div
            style={{
              fontSize: 78,
              lineHeight: 1.08,
              fontWeight: 600,
              letterSpacing: "-0.04em",
              opacity: line2,
              transform: `translateY(${y2}px)`,
              color: accent,
            }}
          >
            on your machine.
          </div>
        </div>
        <div
          style={{
            width: 200 * rule,
            height: 3,
            borderRadius: 2,
            background: accent,
            opacity: 0.8,
            marginTop: 32,
          }}
        />
        <div style={{ display: "flex", gap: 14, marginTop: 58 }}>
          {chips.map((c, i) => (
            <Pill
              key={c.label}
              label={c.label}
              p={ramp(f, c.at, c.at + 22)}
              y={
                ramp(f, c.at, c.at + 22, 16, 0) +
                Math.sin((f - c.at) / 26 + i) * 1.8 * ramp(f, c.at + 12, c.at + 28)
              }
            />
          ))}
        </div>
        <div
          style={{
            marginTop: 54,
            fontFamily: mono,
            fontSize: 15,
            color: tertiary,
            opacity: db,
          }}
        >
          ~/.local/share/daymark/daymark.db
        </div>
      </div>
    </AbsoluteFill>
  );
};

/* --------------------------------------------------------------- history */

const HistoryWindow: React.FC<{ f: number }> = ({ f }) => {
  const rise = spring({
    frame: f,
    fps: 60,
    config: { damping: 24, stiffness: 90, mass: 1.1 },
  });
  const opacity = ramp(f, 0, 26);
  const scale =
    (0.98 + 0.02 * Math.min(1, rise)) *
    (1 + interpolate(f, [0, 180], [0, 0.05], cl));
  const secs = interpolate(f, [4, 172], [0, 274080], {
    ...cl,
    easing: Easing.bezier(0.24, 1, 0.32, 1),
  });
  const showTotal = f > 4;
  const totalH = Math.floor(secs / 3600);
  const totalM = Math.floor((secs % 3600) / 60);
  const hover = ramp(f, 104, 118);
  const cursorX = interpolate(f, [78, 104], [1060, 640], {
    ...cl,
    easing: easeInOut,
  });
  const cursorY = interpolate(f, [78, 104], [700, 236], {
    ...cl,
    easing: easeInOut,
  });
  const cursorIn = f > 74;

  return (
    <div
      style={{
        position: "absolute",
        left: 200,
        top: 214,
        width: 1120,
        height: 780,
        transform: `translate(${-12 * (f / 180)}px, ${
          (1 - Math.min(1, rise)) * 60 - 8 * (f / 180)
        }px) scale(${scale})`,
        transformOrigin: "30% 25%",
        opacity,
        borderRadius: 20,
        background: cream,
        border: "1px solid rgba(38,34,29,0.10)",
        boxShadow: "0 50px 140px rgba(38,34,29,0.20), 0 10px 34px rgba(38,34,29,0.08)",
        overflow: "hidden",
        fontFamily: font,
        color: ink,
      }}
    >
      <Chrome />
      <div style={{ padding: "24px 40px 0" }}>
        <div
          style={{
            fontSize: 11,
            fontWeight: 600,
            letterSpacing: "1.1px",
            color: secondary,
          }}
        >
          LAST 14 DAYS
        </div>
        <div
          style={{
            height: 60,
            display: "flex",
            alignItems: "center",
            fontSize: 48,
            fontWeight: 600,
            letterSpacing: "-1.1px",
            marginTop: 2,
          }}
        >
          {showTotal ? `${totalH}h ${totalM}m` : "0m"}
        </div>
        <div style={{ fontSize: 13, color: secondary }}>
          Across 13 days · 5h 51m a day
        </div>
        <div style={{ marginTop: 24 }}>
          {HISTORY.map((row, i) => {
            const p = spring({
              frame: f - (12 + i * 4),
              fps: 60,
              config: { damping: 22, stiffness: 120, mass: 0.9 },
            });
            return (
              <div
                key={row.day}
                style={{
                  height: 52,
                  display: "flex",
                  alignItems: "center",
                  gap: 10,
                  opacity: Math.min(1, p),
                  transform: `translateX(${(1 - Math.min(1, p)) * 22}px)`,
                  background: row.today
                    ? `rgba(242,235,225,${0.85 * hover})`
                    : "transparent",
                  borderRadius: 9,
                }}
              >
                <div
                  style={{
                    width: 92,
                    fontSize: 13,
                    fontWeight: row.today ? 600 : 500,
                    color: row.today ? ink : secondary,
                  }}
                >
                  {row.day}
                </div>
                <div style={{ width: 72, fontSize: 11, color: tertiary }}>
                  {row.date}
                </div>
                <div
                  style={{
                    flex: 1,
                    height: 4,
                    borderRadius: 2,
                    background: track,
                    marginRight: 12,
                  }}
                >
                  <div
                    style={{
                      width: `${row.ratio * 100}%`,
                      height: "100%",
                      borderRadius: 2,
                      background: row.today ? accent : barHover,
                      transform: `scaleX(${Math.min(1, p)})`,
                      transformOrigin: "left center",
                    }}
                  />
                </div>
                <div
                  style={{
                    width: 52,
                    textAlign: "right",
                    fontSize: 12,
                    fontWeight: 500,
                    color: secondary,
                  }}
                >
                  {row.time}
                </div>
                <div style={{ width: 20, color: tertiary, fontSize: 16 }}>›</div>
              </div>
            );
          })}
        </div>
      </div>
      {cursorIn ? (
        <Cursor
          f={f}
          scale={1}
          x={cursorX}
          y={cursorY}
          click={104}
        />
      ) : null}
    </div>
  );
};

const HistoryScene: React.FC = () => {
  const f = useCurrentFrame();
  const head = ramp(f, 2, 30);
  const sub = ramp(f, 12, 38);

  return (
    <AbsoluteFill style={{ background: cream, fontFamily: font, overflow: "hidden" }}>
      <div
        style={{
          position: "absolute",
          inset: 0,
          background:
            `radial-gradient(1100px 760px at 10% -8%, ${creamHigh} 0%, rgba(255,250,243,0) 60%),` +
            "radial-gradient(900px 700px at 104% 110%, #eee3d3 0%, rgba(238,227,211,0) 55%)",
        }}
      />
      <div
        style={{
          position: "absolute",
          left: 144,
          top: 86,
          opacity: head,
          fontSize: 58,
          fontWeight: 600,
          letterSpacing: "-0.035em",
        }}
      >
        A longer view.
      </div>
      <div
        style={{
          position: "absolute",
          left: 146,
          top: 156,
          opacity: sub,
          fontSize: 25,
          color: secondary,
        }}
      >
        Two weeks at a glance, and the days you actually used.
      </div>
      <DriftOrb f={f} x={-140} y={-200} size={900} opacity={0.09} />
      <HistoryWindow f={f} />
    </AbsoluteFill>
  );
};

/* ------------------------------------------------------------------ hour */

const HourScene: React.FC = () => {
  const f = useCurrentFrame();
  const head = ramp(f, 0, 26);
  const sub = ramp(f, 8, 34);
  const push = interpolate(f, [0, 220], [1, 1.055], cl);
  const scan = interpolate(f, [0, 220], [-240, 1760], cl);
  const peak = ramp(f, 54, 86);
  const total = ramp(f, 40, 70);
  const insight = ramp(f, 86, 112);
  const pitch = 65;
  const barW = 47;
  const baseY = 360;
  const maxH = 340;

  return (
    <AbsoluteFill style={{ background: cream, fontFamily: font, overflow: "hidden" }}>
      <div
        style={{
          position: "absolute",
          inset: 0,
          background:
            `radial-gradient(1200px 760px at 8% -10%, ${creamHigh} 0%, rgba(255,250,243,0) 60%),` +
            "radial-gradient(900px 700px at 104% 112%, #eee3d3 0%, rgba(238,227,211,0) 55%)",
        }}
      />
      <DriftOrb f={f} x={-180} y={-260} size={820} opacity={0.05} />
      <div
        style={{
          position: "absolute",
          inset: 0,
          transform: `scale(${push})`,
          transformOrigin: "50% 55%",
        }}
      >
        <div
          style={{
            position: "absolute",
            left: 144,
            top: 82,
            fontSize: 64,
            fontWeight: 600,
            letterSpacing: "-0.035em",
            opacity: head,
          }}
        >
          The rhythm of the day.
        </div>
        <div
          style={{
            position: "absolute",
            left: 146,
            top: 166,
            fontSize: 25,
            color: secondary,
            opacity: sub,
          }}
        >
          Where the hours actually went.
        </div>
        <div
          style={{
            position: "absolute",
            left: 144,
            top: 238,
            width: 120 * ramp(f, 20, 50),
            height: 3,
            borderRadius: 2,
            background: accent,
          }}
        />
        <div
          style={{
            position: "absolute",
            right: 144,
            top: 246,
            fontSize: 20,
            color: tertiary,
            opacity: total,
          }}
        >
          <span style={{ color: ink, fontWeight: 600 }}>6h 44m</span> today
        </div>

        <div
          style={{
            position: "absolute",
            left: 144,
            top: 300,
            width: 1542,
            height: baseY + 60,
          }}
        >
          {[0.25, 0.5, 0.75].map((g) => (
            <div
              key={g}
              style={{
                position: "absolute",
                left: 0,
                top: baseY - maxH * g,
                width: 1542,
                height: 1,
                background: "rgba(38,34,29,0.06)",
              }}
            />
          ))}
          <div
            style={{
              position: "absolute",
              left: 9 * pitch - 10,
              top: -4,
              width: pitch * 2,
              height: baseY + 4,
              borderRadius: 12,
              background: "rgba(170,84,49,0.06)",
              opacity: peak,
            }}
          />
          <div
            style={{
              position: "absolute",
              left: 9 * pitch,
              top: -40,
              fontSize: 13,
              fontWeight: 600,
              letterSpacing: "0.08em",
              color: accent,
              opacity: peak,
              whiteSpace: "nowrap",
            }}
          >
            PEAK · 9–10 AM
          </div>
          <div
            style={{
              position: "absolute",
              left: scan,
              top: -10,
              width: 170,
              height: 390,
              background:
                "linear-gradient(90deg, rgba(255,255,255,0) 0%, rgba(255,255,255,0.38) 50%, rgba(255,255,255,0) 100%)",
              opacity: 0.55,
            }}
          />
          {HOURS.map((v, i) => {
            const p = spring({
              frame: f - (10 + i * 2.4),
              fps: 60,
              config: { damping: 18, stiffness: 110, mass: 0.8 },
            });
            const h = v * maxH * Math.min(1, p);
            return (
              <div
                key={i}
                style={{
                  position: "absolute",
                  left: i * pitch,
                  top: baseY - h,
                  width: barW,
                  height: h,
                  borderRadius: 4,
                  background: accent,
                  opacity: v > 0 ? 0.82 : 0,
                }}
              />
            );
          })}
          <div
            style={{
              position: "absolute",
              left: 0,
              top: baseY,
              width: 1542,
              height: 1,
              background: line,
            }}
          />
          {[
            { t: "12 AM", x: 0, align: "left" as const },
            { t: "6 AM", x: (pitch * 6 + barW / 2) / 1542, align: "center" as const },
            { t: "12 PM", x: (pitch * 12 + barW / 2) / 1542, align: "center" as const },
            { t: "6 PM", x: (pitch * 18 + barW / 2) / 1542, align: "center" as const },
            { t: "12 AM", x: 1, align: "right" as const },
          ].map((l, i) => (
            <div
              key={i}
              style={{
                position: "absolute",
                top: baseY + 14,
                left: l.align === "left" ? 0 : l.align === "right" ? "100%" : `${l.x * 100}%`,
                transform:
                  l.align === "right"
                    ? "translateX(-100%)"
                    : l.align === "center"
                      ? "translateX(-50%)"
                      : undefined,
                fontSize: 13,
                color: tertiary,
                whiteSpace: "nowrap",
              }}
            >
              {l.t}
            </div>
          ))}
        </div>

        <div
          style={{
            position: "absolute",
            left: 144,
            top: 806,
            display: "flex",
            alignItems: "center",
            gap: 18,
            opacity: insight,
          }}
        >
          <div style={{ width: 3, height: 58, borderRadius: 2, background: accent }} />
          <div>
            <div style={{ fontSize: 30, fontWeight: 600, letterSpacing: "-0.02em" }}>
              Nine to ten in the morning carried the day.
            </div>
            <div style={{ fontSize: 18, color: secondary, marginTop: 5 }}>
              11 active hours · 6h 44m in total
            </div>
          </div>
        </div>
      </div>
    </AbsoluteFill>
  );
};

/* ------------------------------------------------------------------- end */

const EndCard: React.FC = () => {
  const f = useCurrentFrame();
  const mark = spring({
    frame: f,
    fps: 60,
    config: { damping: 20, stiffness: 110, mass: 0.9 },
  });
  const word = ramp(f, 6, 30);
  const wordY = ramp(f, 6, 30, 18, 0);
  const tag = ramp(f, 16, 40);
  const chip = ramp(f, 26, 48);
  const fine = ramp(f, 34, 54);
  const push = interpolate(f, [0, 120], [1, 1.04], cl);
  const caretOn = Math.floor(f / 26) % 2 === 0;

  return (
    <AbsoluteFill style={{ background: cream, fontFamily: font, overflow: "hidden" }}>
      <div
        style={{
          position: "absolute",
          inset: 0,
          background:
            `radial-gradient(1000px 800px at 50% 55%, rgba(170,84,49,0.07) 0%, rgba(170,84,49,0) 60%),` +
            `radial-gradient(1200px 800px at 10% -10%, ${creamHigh} 0%, rgba(255,250,243,0) 60%)`,
        }}
      />
      <DriftOrb f={f} x={380} y={-180} size={900} opacity={0.1} />
      <div
        style={{
          position: "absolute",
          inset: 0,
          display: "flex",
          flexDirection: "column",
          alignItems: "center",
          justifyContent: "center",
          paddingBottom: 24,
          transform: `scale(${push}) translateY(${-8 * (f / 120)}px)`,
        }}
      >
        <Img
          src={staticFile("icon.svg")}
          style={{
            width: 92,
            height: 92,
            opacity: Math.min(1, mark),
            transform: `scale(${0.86 + 0.14 * Math.min(1, mark)})`,
            filter: "drop-shadow(0 16px 34px rgba(170,84,49,0.20))",
          }}
        />
        <div
          style={{
            marginTop: 30,
            fontSize: 86,
            fontWeight: 600,
            letterSpacing: "-0.045em",
            lineHeight: 1,
            opacity: word,
            transform: `translateY(${wordY}px)`,
          }}
        >
          Daymark
        </div>
        <div style={{ marginTop: 18, fontSize: 25, color: secondary, opacity: tag }}>
          A quiet screen time journal for GNOME
        </div>
        <div
          style={{
            marginTop: 38,
            height: 56,
            display: "flex",
            alignItems: "center",
            gap: 16,
            padding: "0 22px",
            borderRadius: 12,
            background: surface,
            border: `1px solid ${line}`,
            boxShadow: "0 10px 30px rgba(38,34,29,0.08)",
            fontFamily: mono,
            fontSize: 22,
            opacity: chip,
          }}
        >
          <span style={{ color: tertiary }}>$</span>
          <span>./install.sh</span>
          <span
            style={{
              width: 2,
              height: 22,
              marginLeft: 6,
              background: accent,
              opacity: caretOn ? 0.8 : 0,
            }}
          />
        </div>
        <div
          style={{
            marginTop: 24,
            fontSize: 15,
            letterSpacing: "0.15em",
            color: tertiary,
            opacity: fine,
          }}
        >
          FREE AND OPEN SOURCE · GNOME 45–50 · LOCAL ONLY
        </div>
      </div>
    </AbsoluteFill>
  );
};

/* ------------------------------------------------------------------ main */

export const DaymarkTour: React.FC = () => {
  return (
    <AbsoluteFill style={{ background: cream, fontFamily: font }}>
      <Sequence from={0} durationInFrames={150}>
        <Hook />
      </Sequence>
      <Sequence from={140} durationInFrames={720}>
        <DashboardScene />
      </Sequence>
      <Sequence from={860} durationInFrames={220}>
        <HourScene />
      </Sequence>
      <Sequence from={1080} durationInFrames={180}>
        <Privacy />
      </Sequence>
      <Sequence from={1260} durationInFrames={180}>
        <HistoryScene />
      </Sequence>
      <Sequence from={1440} durationInFrames={120}>
        <EndCard />
      </Sequence>
    </AbsoluteFill>
  );
};
