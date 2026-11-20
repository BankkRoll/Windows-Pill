/**
 * The mockups as the landing page places them: the hero's notch opening
 * into the panel, and still shots of the open panel on a given tab.
 *
 * Every mockup is a picture of the app, not a working copy, so it is inert
 * (no focus, no clicks) and carries one description for screen readers.
 */

import { useEffect, useState, type ReactNode } from "react";
import { SHAPES, type ShapeMode } from "../lib/shapes";
import { AgentsTab } from "./Agents";
import { CollapsedContent, ExpandedPanel, type PanelTab } from "./Panel";
import { SecurityTab } from "./Security";
import { Desktop, Notch, Scaled, useMorph, useReducedMotion } from "./Stage";
import { TodayTab } from "./Today";

/** Desktop height: the open panel plus a strip of wallpaper below it. */
const STAGE_HEIGHT = SHAPES.expanded.height + 56;
/** The open notch with its shoulders, plus a margin of wallpaper each side. */
const MIN_WIDTH = SHAPES.expanded.width + SHAPES.expanded.shoulder * 2 + 24;

/** Props for {@link Figure}. */
interface FigureProps {
  label: string;
  children: ReactNode;
  className?: string;
}

function Figure({ label, children, className = "" }: FigureProps) {
  return (
    <figure role="img" aria-label={label} className={`m-0 select-none ${className}`}>
      <div inert>{children}</div>
    </figure>
  );
}

/**
 * The hero: the collapsed notch, which springs open into the Today panel a
 * moment after it scrolls into view. With reduced motion it is shown open.
 */
export function HeroNotch() {
  const reduced = useReducedMotion();
  const [mode, setMode] = useState<ShapeMode>(reduced ? "expanded" : "peek");
  const [content, setContent] = useState<"collapsed" | "expanded">(reduced ? "expanded" : "collapsed");
  const geometry = useMorph(mode, reduced);

  useEffect(() => {
    if (reduced) {
      setMode("expanded");
      setContent("expanded");
      return;
    }
    // As in the app: the shape starts growing at once while the collapsed
    // content fades out, and the panel fades in shortly after.
    const open = window.setTimeout(() => setMode("expanded"), 1100);
    const swap = window.setTimeout(() => setContent("expanded"), 1180);
    return () => {
      window.clearTimeout(open);
      window.clearTimeout(swap);
    };
  }, [reduced]);

  return (
    <Figure label="Pill on a Windows desktop: the small notch at the top of the screen opens into a panel showing Now playing, Windows notifications and today's tasks across three projects.">
      <Scaled minWidth={MIN_WIDTH} maxWidth={1100} height={STAGE_HEIGHT}>
        {(width) => (
          <Desktop width={width} height={STAGE_HEIGHT}>
            <Notch geometry={geometry} stageWidth={width}>
              <div
                key={content}
                className="absolute top-0 left-1/2 -translate-x-1/2 animate-[fade-in_140ms_ease-out_both] motion-reduce:animate-none"
              >
                {content === "collapsed" ? (
                  <CollapsedContent />
                ) : (
                  <ExpandedPanel active="today">
                    <TodayTab />
                  </ExpandedPanel>
                )}
              </div>
            </Notch>
          </Desktop>
        )}
      </Scaled>
    </Figure>
  );
}

/** Props for {@link PanelShot}. */
export interface PanelShotProps {
  tab: PanelTab;
  /** What the shot shows, for screen readers. */
  label: string;
}

/** The open panel on one tab, hanging from the top of a desktop. */
export function PanelShot({ tab, label }: PanelShotProps) {
  const view =
    tab === "agents" ? (
      <ExpandedPanel active="agents">
        <AgentsTab />
      </ExpandedPanel>
    ) : tab === "security" ? (
      <ExpandedPanel active="security" project={{ name: "storefront", color: "#2F7BFF" }} commandBar={false}>
        <SecurityTab />
      </ExpandedPanel>
    ) : (
      <ExpandedPanel active="today">
        <TodayTab />
      </ExpandedPanel>
    );
  return (
    <Figure label={label}>
      <Scaled minWidth={MIN_WIDTH} maxWidth={MIN_WIDTH + 80} height={STAGE_HEIGHT}>
        {(width) => (
          <Desktop width={width} height={STAGE_HEIGHT}>
            <Notch geometry={SHAPES.expanded} stageWidth={width}>
              <div className="absolute top-0 left-1/2 -translate-x-1/2">{view}</div>
            </Notch>
          </Desktop>
        )}
      </Scaled>
    </Figure>
  );
}

/**
 * One of the app's cards at its real size on a tile of the notch's glass,
 * for showing the Today tab's pieces one by one. The cards are fluid, as in
 * the app, so they fill the tile.
 *
 * @param props.label - What the card shows, for screen readers.
 */
export function CardShot({ label, children }: { label: string; children: ReactNode }) {
  return (
    <Figure label={label}>
      <div className="rounded-[20px] p-2 text-[13px] text-white/92 ring-1 ring-white/[0.08]" style={{ background: "rgb(22 22 25)" }}>
        {children}
      </div>
    </Figure>
  );
}
