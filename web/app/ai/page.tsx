import { getIdentities } from "@/lib/db";
import AiChat from "./AiChat";

export const dynamic = "force-dynamic";

export default function AiPage() {
  const identities = getIdentities();
  return <AiChat identities={identities} />;
}
