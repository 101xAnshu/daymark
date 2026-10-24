import { Composition } from "remotion";
import { DaymarkTour } from "./Tour";

export const RemotionRoot = () => {
  return (
    <Composition
      id="DaymarkTour"
      component={DaymarkTour}
      durationInFrames={1560}
      fps={60}
      width={1920}
      height={1080}
    />
  );
};
