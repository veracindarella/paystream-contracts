import Dashboard from "@/components/Dashboard";
import { assertContractIdConfigured } from "@/lib/stellar";

assertContractIdConfigured();

export default function Page() {
  return <Dashboard />;
}