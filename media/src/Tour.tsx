import React from "react";
import {
  AbsoluteFill,
  Easing,
  Img,
  Sequence,
  interpolate,
  staticFile,
  useCurrentFrame,
} from "remotion";

const cream = "#f7f3ec";
const creamHigh = "#fffaf3";
const ink = "#26221d";
const secondary = "#6b6258";
const tertiary = "#72685c";
const accent = "#aa5431";
const accentSoft = "#f3e1d5";
const dark = "#161411";
const darkLine = "#3d362e";
const darkText = "#f3eee6";
const darkSecondary = "#b6aa9b";

const font =
  '"Ubuntu Sans", Ubuntu, Cantarell, "DejaVu Sans", "Noto Sans", sans-serif';
const mono = '"Ubuntu Mono", "DejaVu Sans Mono", monospace';

const clamp = {
  extrapolateLeft: "clamp" as const,
  extrapolateRight: "clamp" as const,
};
const ease = Easing.bezier(0.22, 1, 0.36, 1);

const enter = (f: number, start: number, duration: number) =>
  interpolate(f, [start, start + duration], [0, 1], { ...clamp, easing: ease });

const leave = (f: number, start: number, duration: number) =>
  interpolate(f, [start, start + duration], [1, 0], { ...clamp, easing: ease });

const envelope = (
  f: number,
  inStart: number,
  inDuration: number,
  outStart: number,
  outDuration: number,
) => Math.min(enter(f, inStart, inDuration), leave(f, outStart, outDuration));

const CreamBackdrop: React.FC<{ children: React.ReactNode }> = ({
  children,
}) => (
  <AbsoluteFill style={{ background: cream, fontFamily: font, color: ink }}>
    <div
      style={{
        position: "absolute",
        inset: 0,
        background:
          `radial-gradient(1200px 820px at 16% -6%, ${creamHigh} 0%, rgba(255,250,243,0) 62%),` +
          "radial-gradient(1000px 720px at 104% 108%, #eee3d5 0%, rgba(238,227,213,0) 58%)",
      }}
    />
    {children}
  </AbsoluteFill>
);

const DarkBackdrop: React.FC<{ children: React.ReactNode }> = ({
  children,
}) => (
  <AbsoluteFill style={{ background: dark, fontFamily: font, color: darkText }}>
    <div
      style={{
        position: "absolute",
        inset: 0,
        background:
          "radial-gradient(900px 620px at 50% 44%, rgba(170,84,49,0.16) 0%, rgba(170,84,49,0) 68%)",
      }}
    />
    {children}
  </AbsoluteFill>
);

type WindowProps = {
  src: string;
  width: number;
  left: number;
  top: number;
  scale: number;
  opacity: number;
  rotate?: number;
  shadow?: string;
  children?: React.ReactNode;
};

const AppWindow: React.FC<WindowProps> = ({
  src,
  width,
  left,
  top,
  scale,
  opacity,
  rotate = 0,
  shadow = "0 44px 120px rgba(38,34,29,0.24), 0 8px 28px rgba(38,34,29,0.12)",
  children,
}) => {
  const height = (width / 1120) * 780;
  return (
    <div
      style={{
        position: "absolute",
        left,
        top,
        width,
        height,
        opacity,
        transform: `translateZ(0) scale(${scale}) rotate(${rotate}deg)`,
        transformOrigin: "50% 40%",
        borderRadius: 20,
        overflow: "hidden",
        boxShadow: shadow,
        border: "1px solid rgba(38,34,29,0.10)",
        background: cream,
      }}
    >
      <Img
        src={staticFile(src)}
        style={{ width: "100%", height: "100%", display: "block" }}
      />
      {children}
    </div>
  );
};

const Intro: React.FC = () => {
  const f = useCurrentFrame();
  // The first frame is a complete title card, so the README GIF starts on a
  // finished image instead of a blank cream frame. Motion begins from there.
  const opacity = leave(f, 94, 18);
  const iconScale = interpolate(f, [0, 30], [0.92, 1], {
    ...clamp,
    easing: ease,
  });
  const iconOpacity = 1;
  const word = 1;
  const wordY = 0;
  const rule = 1;
  const tag = 1;
  const tagY = interpolate(f, [6, 34], [12, 0], { ...clamp, easing: ease });

  return (
    <CreamBackdrop>
      <AbsoluteFill
        style={{
          alignItems: "center",
          justifyContent: "center",
          opacity,
          paddingBottom: 30,
        }}
      >
        <Img
          src={staticFile("icon.svg")}
          style={{
            width: 118,
            height: 118,
            transform: `scale(${iconScale})`,
            opacity: iconOpacity,
            filter: "drop-shadow(0 18px 36px rgba(170,84,49,0.22))",
          }}
        />
        <div
          style={{
            marginTop: 38,
            fontSize: 104,
            fontWeight: 600,
            letterSpacing: "-0.038em",
            lineHeight: 1,
            opacity: word,
            transform: `translateY(${wordY}px)`,
          }}
        >
          Daymark
        </div>
        <div
          style={{
            marginTop: 24,
            width: 232 * rule,
            height: 3,
            borderRadius: 2,
            background: accent,
            opacity: 0.9,
          }}
        />
        <div
          style={{
            marginTop: 24,
            fontSize: 30,
            color: secondary,
            letterSpacing: "0.01em",
            opacity: tag,
            transform: `translateY(${tagY}px)`,
          }}
        >
          A quiet screen time journal for GNOME
        </div>
      </AbsoluteFill>
    </CreamBackdrop>
  );
};

const Dashboard: React.FC = () => {
  const f = useCurrentFrame();
  const opacity = envelope(f, 0, 18, 128, 16);
  const head = enter(f, 4, 22);
  const headY = interpolate(f, [4, 26], [22, 0], { ...clamp, easing: ease });
  const kicker = enter(f, 0, 16);
  const windowIn = enter(f, 10, 28);
  const windowY = interpolate(f, [10, 38], [46, 0], {
    ...clamp,
    easing: ease,
  });
  const slowZoom = interpolate(f, [20, 144], [1, 1.028]);
  const ring = enter(f, 34, 20);

  return (
    <CreamBackdrop>
      <AbsoluteFill style={{ opacity }}>
        <div
          style={{
            position: "absolute",
            left: 160,
            top: 104,
            opacity: kicker,
            fontSize: 15,
            fontWeight: 600,
            letterSpacing: "0.22em",
            color: accent,
          }}
        >
          A DAY, AT A GLANCE
        </div>
        <div
          style={{
            position: "absolute",
            left: 160,
            top: 136,
            fontSize: 62,
            fontWeight: 600,
            letterSpacing: "-0.03em",
            lineHeight: 1.05,
            opacity: head,
            transform: `translateY(${headY}px)`,
          }}
        >
          See where the day went.
        </div>

        <div
          style={{
            position: "absolute",
            left: 360,
            top: 240,
            width: 1200,
            height: (1200 / 1120) * 780,
            opacity: windowIn,
            transform: `translateY(${windowY}px) scale(${slowZoom})`,
            transformOrigin: "50% 40%",
            borderRadius: 20,
            overflow: "hidden",
            boxShadow:
              "0 54px 140px rgba(38,34,29,0.26), 0 10px 34px rgba(38,34,29,0.12)",
            border: "1px solid rgba(38,34,29,0.10)",
            background: cream,
          }}
        >
          <Img
            src={staticFile("dashboard.png")}
            style={{ width: "100%", height: "100%", display: "block" }}
          />
          <div
            style={{
              position: "absolute",
              left: 36,
              top: 176,
              width: 208 * ring,
              height: 3,
              borderRadius: 2,
              background: accent,
              opacity: ring * 0.9,
            }}
          />
        </div>
      </AbsoluteFill>
    </CreamBackdrop>
  );
};

const Zoom: React.FC = () => {
  const f = useCurrentFrame();
  const opacity = envelope(f, 0, 12, 96, 16);
  const scale = interpolate(f, [0, 112], [1.02, 1.34], {
    ...clamp,
    easing: Easing.bezier(0.3, 0.1, 0.2, 1),
  });
  const caption = enter(f, 30, 22);
  const captionY = interpolate(f, [30, 52], [16, 0], {
    ...clamp,
    easing: ease,
  });

  return (
    <CreamBackdrop>
      <AbsoluteFill style={{ opacity }}>
        <div
          style={{
            position: "absolute",
            left: 0,
            top: 54,
            width: 1920,
            height: 900,
            overflow: "hidden",
          }}
        >
          <div
            style={{
              position: "absolute",
              left: 260,
              top: 0,
              width: 1400,
              height: (1400 / 1120) * 780,
              transform: `scale(${scale})`,
              transformOrigin: "28% 66%",
              borderRadius: 22,
              overflow: "hidden",
              boxShadow: "0 40px 120px rgba(38,34,29,0.22)",
              border: "1px solid rgba(38,34,29,0.10)",
              background: cream,
            }}
          >
            <Img
              src={staticFile("dashboard.png")}
              style={{ width: "100%", height: "100%", display: "block" }}
            />
          </div>
          <div
            style={{
              position: "absolute",
              inset: 0,
              background:
                "linear-gradient(180deg, rgba(247,243,236,0.55) 0%, rgba(247,243,236,0) 18%, rgba(22,20,17,0) 72%, rgba(22,20,17,0.18) 100%)",
            }}
          />
        </div>

        <div
          style={{
            position: "absolute",
            left: 0,
            right: 0,
            bottom: 0,
            height: 128,
            background: cream,
            borderTop: "1px solid rgba(38,34,29,0.10)",
            display: "flex",
            alignItems: "center",
            paddingLeft: 160,
            opacity: caption,
          }}
        >
          <div
            style={{
              width: 34,
              height: 3,
              borderRadius: 2,
              background: accent,
              marginRight: 22,
            }}
          />
          <div
            style={{
              fontSize: 46,
              fontWeight: 600,
              letterSpacing: "-0.025em",
              transform: `translateY(${captionY}px)`,
            }}
          >
            Every app, accounted for.
          </div>
        </div>
      </AbsoluteFill>
    </CreamBackdrop>
  );
};

const Chip: React.FC<{
  label: string;
  progress: number;
  y: number;
}> = ({ label, progress, y }) => (
  <div
    style={{
      display: "flex",
      alignItems: "center",
      gap: 12,
      height: 62,
      padding: "0 26px",
      borderRadius: 999,
      border: `1px solid ${darkLine}`,
      background: "rgba(30,26,22,0.72)",
      color: darkSecondary,
      fontSize: 23,
      letterSpacing: "0.01em",
      opacity: progress,
      transform: `translateY(${y}px)`,
    }}
  >
    <div
      style={{
        width: 7,
        height: 7,
        borderRadius: 4,
        background: accent,
        opacity: 0.9,
      }}
    />
    {label}
  </div>
);

const Privacy: React.FC = () => {
  const f = useCurrentFrame();
  const opacity = envelope(f, 0, 16, 92, 16);
  const line1 = enter(f, 2, 22);
  const line2 = enter(f, 8, 22);
  const y1 = interpolate(f, [2, 24], [26, 0], { ...clamp, easing: ease });
  const y2 = interpolate(f, [8, 30], [26, 0], { ...clamp, easing: ease });
  const rule = enter(f, 22, 20);
  const chips = [
    { label: "No account", start: 30 },
    { label: "No cloud", start: 36 },
    { label: "No telemetry", start: 42 },
  ];

  return (
    <DarkBackdrop>
      <AbsoluteFill
        style={{ opacity, alignItems: "center", justifyContent: "center" }}
      >
        <div style={{ marginTop: -40 }}>
          <div
            style={{
              fontSize: 76,
              fontWeight: 600,
              letterSpacing: "-0.035em",
              lineHeight: 1.08,
              color: darkText,
              opacity: line1,
              transform: `translateY(${y1}px)`,
            }}
          >
            Your day stays
          </div>
          <div
            style={{
              fontSize: 76,
              fontWeight: 600,
              letterSpacing: "-0.035em",
              lineHeight: 1.08,
              color: accent,
              opacity: line2,
              transform: `translateY(${y2}px)`,
            }}
          >
            on your machine.
          </div>
          <div
            style={{
              marginTop: 34,
              width: 190 * rule,
              height: 3,
              borderRadius: 2,
              background: accent,
              opacity: 0.75,
            }}
          />
        </div>

        <div
          style={{
            position: "absolute",
            top: 704,
            display: "flex",
            gap: 14,
          }}
        >
          {chips.map((chip) => (
            <Chip
              key={chip.label}
              label={chip.label}
              progress={enter(f, chip.start, 20)}
              y={interpolate(f, [chip.start, chip.start + 20], [14, 0], {
                ...clamp,
                easing: ease,
              })}
            />
          ))}
        </div>
      </AbsoluteFill>
    </DarkBackdrop>
  );
};

const History: React.FC = () => {
  const f = useCurrentFrame();
  const opacity = envelope(f, 0, 16, 92, 16);
  const head = enter(f, 2, 22);
  const left = enter(f, 10, 24);
  const right = enter(f, 18, 24);
  const leftX = interpolate(f, [10, 34], [-56, 0], { ...clamp, easing: ease });
  const rightX = interpolate(f, [18, 42], [56, 0], { ...clamp, easing: ease });
  const label = enter(f, 34, 20);

  return (
    <CreamBackdrop>
      <AbsoluteFill style={{ opacity }}>
        <div
          style={{
            position: "absolute",
            left: 160,
            top: 96,
            fontSize: 56,
            fontWeight: 600,
            letterSpacing: "-0.03em",
            opacity: head,
          }}
        >
          A longer view.
        </div>
        <div
          style={{
            position: "absolute",
            left: 162,
            top: 164,
            fontSize: 26,
            color: secondary,
            opacity: enter(f, 14, 22),
          }}
        >
          Two weeks at a glance, and the knobs that matter.
        </div>

        <div
          style={{
            position: "absolute",
            left: 150,
            top: 300,
            width: 780,
            height: 600,
            opacity: left,
            transform: `translateX(${leftX}px) rotate(-1.6deg)`,
            transformOrigin: "50% 50%",
          }}
        >
          <div
            style={{
              position: "absolute",
              left: 14,
              top: -38,
              fontSize: 22,
              color: secondary,
              whiteSpace: "nowrap",
              opacity: label,
            }}
          >
            Fourteen days at a glance
          </div>
          <AppWindow
            src="history.png"
            width={780}
            left={0}
            top={0}
            scale={1}
            opacity={1}
            shadow="0 34px 90px rgba(38,34,29,0.18)"
          />
        </div>

        <div
          style={{
            position: "absolute",
            left: 1032,
            top: 332,
            width: 780,
            height: 600,
            opacity: right,
            transform: `translateX(${rightX}px) rotate(1.6deg)`,
            transformOrigin: "50% 50%",
          }}
        >
          <div
            style={{
              position: "absolute",
              left: 14,
              top: -38,
              fontSize: 22,
              color: secondary,
              whiteSpace: "nowrap",
              opacity: label,
            }}
          >
            Yours to tune
          </div>
          <AppWindow
            src="settings.png"
            width={780}
            left={0}
            top={0}
            scale={1}
            opacity={1}
            shadow="0 34px 90px rgba(38,34,29,0.18)"
          />
        </div>
      </AbsoluteFill>
    </CreamBackdrop>
  );
};

const End: React.FC = () => {
  const f = useCurrentFrame();
  const icon = enter(f, 0, 22);
  const iconY = interpolate(f, [0, 24], [18, 0], { ...clamp, easing: ease });
  const word = enter(f, 8, 24);
  const wordY = interpolate(f, [8, 32], [18, 0], { ...clamp, easing: ease });
  const tag = enter(f, 18, 22);
  const install = enter(f, 28, 22);
  const fine = enter(f, 38, 20);

  return (
    <CreamBackdrop>
      <AbsoluteFill
        style={{
          alignItems: "center",
          justifyContent: "center",
          paddingBottom: 24,
        }}
      >
        <Img
          src={staticFile("icon.svg")}
          style={{
            width: 92,
            height: 92,
            opacity: icon,
            transform: `translateY(${iconY}px)`,
            filter: "drop-shadow(0 14px 30px rgba(170,84,49,0.18))",
          }}
        />
        <div
          style={{
            marginTop: 30,
            fontSize: 84,
            fontWeight: 600,
            letterSpacing: "-0.04em",
            lineHeight: 1,
            opacity: word,
            transform: `translateY(${wordY}px)`,
          }}
        >
          Daymark
        </div>
        <div
          style={{
            marginTop: 18,
            fontSize: 26,
            color: secondary,
            opacity: tag,
          }}
        >
          A quiet screen time journal for GNOME
        </div>

        <div
          style={{
            marginTop: 40,
            display: "flex",
            alignItems: "center",
            gap: 16,
            height: 56,
            padding: "0 22px",
            borderRadius: 12,
            background: "#fffdf9",
            border: "1px solid #e8dfd3",
            color: ink,
            fontFamily: mono,
            fontSize: 22,
            opacity: install,
            boxShadow: "0 10px 30px rgba(38,34,29,0.08)",
          }}
        >
          <span style={{ color: tertiary }}>$</span>
          <span>./install.sh</span>
        </div>

        <div
          style={{
            marginTop: 26,
            fontSize: 17,
            letterSpacing: "0.12em",
            color: tertiary,
            opacity: fine,
          }}
        >
          FREE AND OPEN SOURCE · GNOME 45–50 · LOCAL ONLY
        </div>
      </AbsoluteFill>
    </CreamBackdrop>
  );
};

export const DaymarkTour: React.FC = () => {
  return (
    <AbsoluteFill style={{ background: cream, fontFamily: font }}>
      <Sequence from={0} durationInFrames={112}>
        <Intro />
      </Sequence>
      <Sequence from={112} durationInFrames={144}>
        <Dashboard />
      </Sequence>
      <Sequence from={256} durationInFrames={112}>
        <Zoom />
      </Sequence>
      <Sequence from={368} durationInFrames={108}>
        <Privacy />
      </Sequence>
      <Sequence from={476} durationInFrames={108}>
        <History />
      </Sequence>
      <Sequence from={584} durationInFrames={76}>
        <End />
      </Sequence>
    </AbsoluteFill>
  );
};
