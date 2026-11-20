/**
 * Picks what the glass theme blurs: the live desktop, one of Pill's
 * gradients, a wallpaper that ships with Windows, or any picture on the PC.
 * Choices save immediately; the core re-samples and both the notch and the
 * pop-out update live.
 */
import { Desktop20Regular, ImageAdd20Regular } from "@fluentui/react-icons";
import { open } from "@tauri-apps/plugin-dialog";
import { useEffect, useState, type ReactNode } from "react";
import { attempt } from "../../lib/actions";
import { GLASS_GRADIENTS, glassGradient, gradientPreview, useWallpaper } from "../../lib/backdrop";
import { api, isTauri, type SettingsPatch, type StockWallpaper } from "../../lib/native";
import { usePillStore } from "../../store";
import { Toggle } from "../controls";
import { Row } from "./kit";

const IMAGE_EXTENSIONS = ["jpg", "jpeg", "png"];

// PERF: Settings remounts on every tab switch; the list and the thumbnails
// cost a directory walk and a decode each, so they're kept for the session.
let stockList: Promise<StockWallpaper[]> | null = null;
const thumbs = new Map<string, Promise<string | null>>();
let lastDesktop: string | null = null;

function loadThumbnail(path: string): Promise<string | null> {
  let p = thumbs.get(path);
  if (!p) {
    p = api.imageThumbnail(path).catch(() => null);
    thumbs.set(path, p);
  }
  return p;
}

function useThumbnail(path: string | null): string | null | undefined {
  const [url, setUrl] = useState<string | null | undefined>(undefined);
  useEffect(() => {
    setUrl(undefined);
    if (!path || !isTauri) return;
    let alive = true;
    void loadThumbnail(path).then((u) => alive && setUrl(u));
    return () => {
      alive = false;
    };
  }, [path]);
  return url;
}

function useStockWallpapers(): StockWallpaper[] {
  const [list, setList] = useState<StockWallpaper[]>([]);
  useEffect(() => {
    if (!isTauri) return;
    stockList ??= api.stockWallpapers().catch(() => []);
    let alive = true;
    void stockList.then((l) => alive && setList(l));
    return () => {
      alive = false;
    };
  }, []);
  return list;
}

const samePath = (a: string, b: string) => a.toLowerCase() === b.toLowerCase();

const fileName = (path: string) => path.split(/[\\/]/).pop() ?? path;

/** The glass backdrop picker plus, for the desktop, the gradient fallback switch. */
export function GlassBackdropPicker() {
  const source = usePillStore((s) => s.config.glassSource);
  const gradient = usePillStore((s) => s.config.glassGradient);
  const image = usePillStore((s) => s.config.glassImage);
  const hold = usePillStore((s) => s.hold);
  const wallpaper = useWallpaper();
  const stock = useStockWallpapers();
  const save = (patch: SettingsPatch) => void attempt(() => api.updateSettings(patch));

  const desktop = source === "auto" || source === "wallpaper";
  const pickedStock = source === "image" && image ? stock.find((s) => samePath(s.path, image)) : undefined;
  const custom = image && !stock.some((s) => samePath(s.path, image)) ? image : null;
  const fallback = glassGradient(gradient).label;
  if (wallpaper?.source === "wallpaper") lastDesktop = wallpaper.url;

  const choose = async () => {
    if (!isTauri) return;
    // The dialog takes focus; holding keeps the panel open behind it.
    hold("dialog", true);
    try {
      const picked = await open({
        multiple: false,
        directory: false,
        title: "Choose a glass backdrop",
        filters: [{ name: "Images", extensions: IMAGE_EXTENSIONS }],
      });
      if (typeof picked === "string") save({ glassSource: "image", glassImage: picked });
    } finally {
      hold("dialog", false);
    }
  };

  const hint =
    source === "auto"
      ? wallpaper?.source === "pill"
        ? `Your wallpaper is too dark or flat for glass, so the ${fallback} gradient is in use`
        : "Your desktop wallpaper, blurred"
      : source === "wallpaper"
        ? "Always your desktop wallpaper"
        : source === "pill"
          ? `${fallback}, a Pill gradient that looks the same on every machine`
          : pickedStock
            ? "A wallpaper that ships with Windows"
            : image
              ? fileName(image)
              : "Your desktop wallpaper, until you choose an image";

  return (
    <>
      <div className="flex flex-col gap-2.5 px-3 py-2.5">
        <span className="min-w-0">
          <span className="block text-[12.5px] text-white/85">Glass backdrop</span>
          <span className="block truncate text-[11px] leading-snug text-white/40" title={hint}>
            {hint}
          </span>
        </span>
        <div className="grid grid-cols-4 gap-2" role="radiogroup" aria-label="Glass backdrop">
          <Tile caption="Wallpaper" label="Current wallpaper" selected={desktop} onClick={() => !desktop && save({ glassSource: "auto" })}>
            {lastDesktop ? <Fill url={lastDesktop} /> : <Desktop20Regular className="size-5 text-white/45" />}
          </Tile>
          {GLASS_GRADIENTS.map((g) => (
            <Tile
              key={g.id}
              caption={g.label}
              label={`${g.label} gradient`}
              selected={source === "pill" && gradient === g.id}
              onClick={() => save({ glassSource: "pill", glassGradient: g.id })}
            >
              <Fill url={gradientPreview(g.id)} />
            </Tile>
          ))}
          {stock.length > 0 && (
            <span className="col-span-full pt-1 text-[10.5px] font-medium tracking-wide text-white/40 uppercase">Windows wallpapers</span>
          )}
          {stock.map((s) => (
            <Tile
              key={s.path}
              label={s.name}
              selected={source === "image" && pickedStock?.path === s.path}
              onClick={() => save({ glassSource: "image", glassImage: s.path })}
            >
              <Thumbnail path={s.path} />
            </Tile>
          ))}
          {stock.length > 0 && (
            <span className="col-span-full pt-1 text-[10.5px] font-medium tracking-wide text-white/40 uppercase">Your images</span>
          )}
          {custom && (
            <Tile caption={fileName(custom)} label={fileName(custom)} title={custom} selected={source === "image"} onClick={() => save({ glassSource: "image" })}>
              <Thumbnail path={custom} />
            </Tile>
          )}
          <Tile caption="Choose image…" label="Choose image…" title="Choose a JPEG or PNG on this PC" selected={false} onClick={() => void choose()}>
            <ImageAdd20Regular className="size-5 text-white/55" />
          </Tile>
        </div>
      </div>
      {desktop && (
        <Row label="Use a gradient on dark wallpapers" hint={`Shows ${fallback} when the wallpaper is too dark or flat for glass`}>
          <Toggle checked={source === "auto"} onChange={(on) => save({ glassSource: on ? "auto" : "wallpaper" })} />
        </Row>
      )}
    </>
  );
}

function Tile({
  label,
  caption,
  title,
  selected,
  onClick,
  children,
}: {
  label: string;
  caption?: string;
  title?: string;
  selected: boolean;
  onClick: () => void;
  children: ReactNode;
}) {
  return (
    <button
      type="button"
      role="radio"
      aria-checked={selected}
      aria-label={label}
      title={title ?? label}
      onClick={onClick}
      className={`flex min-w-0 flex-col gap-1 rounded-xl p-1 text-left transition-[background-color,box-shadow] ${
        selected ? "bg-white/7 ring-2 ring-accent" : "ring-1 ring-white/7 hover:bg-white/4"
      }`}
    >
      <span className="relative grid aspect-16/10 place-items-center overflow-hidden rounded-lg bg-white/4">{children}</span>
      {caption && <span className="truncate px-0.5 text-[11px] text-white/70">{caption}</span>}
    </button>
  );
}

function Fill({ url }: { url: string }) {
  return <span className="absolute inset-0 bg-cover bg-center" style={{ backgroundImage: `url(${url})` }} />;
}

function Thumbnail({ path }: { path: string }) {
  const url = useThumbnail(path);
  if (url === undefined) return <span className="absolute inset-0 animate-pulse bg-white/5" />;
  return url ? <Fill url={url} /> : null;
}
