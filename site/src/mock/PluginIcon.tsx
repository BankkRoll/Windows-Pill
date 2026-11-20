/**
 * The fixed icon vocabulary plugins can name, copied from the app's
 * src/components/PluginIcon.tsx.
 */

import {
  Alert16Filled,
  AlertUrgent16Filled,
  ArrowDownload16Filled,
  Bot16Filled,
  BranchFork16Filled,
  Bug16Filled,
  Calendar16Filled,
  Chat16Filled,
  CheckmarkCircle16Filled,
  Clock16Filled,
  Cloud16Filled,
  Code16Filled,
  ErrorCircle16Filled,
  Flash16Filled,
  Globe16Filled,
  Heart16Filled,
  Info16Filled,
  Mail16Filled,
  MusicNote216Filled,
  Pause16Filled,
  Person16Filled,
  Play16Filled,
  Rocket16Filled,
  Shield16Filled,
  Star16Filled,
  Timer16Filled,
  Warning16Filled,
  Wrench16Filled,
  type FluentIcon,
} from "@fluentui/react-icons";

/** An icon name from the plugin protocol. */
export type PluginIconName =
  | "alert"
  | "bell"
  | "bot"
  | "branch"
  | "bug"
  | "calendar"
  | "chat"
  | "check"
  | "clock"
  | "cloud"
  | "code"
  | "download"
  | "error"
  | "flash"
  | "globe"
  | "heart"
  | "info"
  | "mail"
  | "music"
  | "pause"
  | "person"
  | "play"
  | "rocket"
  | "shield"
  | "star"
  | "timer"
  | "warning"
  | "wrench";
/**
 * The fixed icon vocabulary plugins can name. Plugins never supply images or
 * markup; an unknown name never reaches the UI because the core rejects it.
 */
export const PLUGIN_ICONS: Readonly<Record<PluginIconName, FluentIcon>> = {
  alert: AlertUrgent16Filled,
  bell: Alert16Filled,
  bot: Bot16Filled,
  branch: BranchFork16Filled,
  bug: Bug16Filled,
  calendar: Calendar16Filled,
  chat: Chat16Filled,
  check: CheckmarkCircle16Filled,
  clock: Clock16Filled,
  cloud: Cloud16Filled,
  code: Code16Filled,
  download: ArrowDownload16Filled,
  error: ErrorCircle16Filled,
  flash: Flash16Filled,
  globe: Globe16Filled,
  heart: Heart16Filled,
  info: Info16Filled,
  mail: Mail16Filled,
  music: MusicNote216Filled,
  pause: Pause16Filled,
  person: Person16Filled,
  play: Play16Filled,
  rocket: Rocket16Filled,
  shield: Shield16Filled,
  star: Star16Filled,
  timer: Timer16Filled,
  warning: Warning16Filled,
  wrench: Wrench16Filled,
};

/**
 * Draws a plugin icon by name.
 *
 * @param props.icon - Icon name from the plugin protocol.
 * @param props.className - Size and color classes.
 */
export function PluginIcon({ icon, className = "size-3.5" }: { icon: PluginIconName; className?: string }) {
  const Icon = PLUGIN_ICONS[icon];
  return <Icon className={className} aria-hidden />;
}
