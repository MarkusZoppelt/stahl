import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Dialog } from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import type { CreatePositionRequest, Position } from "@/types";
import { Edit, Plus, Trash2 } from "lucide-react";
import { useState } from "react";

interface PositionsProps {
  positions: Position[];
  currency: string;
  onAdd: (request: CreatePositionRequest) => Promise<void>;
  onUpdate: (id: number, data: Partial<Position>) => Promise<void>;
  onDelete: (id: number) => Promise<void>;
}

function formatCurrency(value: number, currency: string): string {
  return new Intl.NumberFormat("en-US", {
    style: "currency",
    currency,
    minimumFractionDigits: 2,
    maximumFractionDigits: 2,
  }).format(value);
}

function formatPercent(value: number | null): string {
  if (value === null) return "-";
  return `${value >= 0 ? "+" : ""}${value.toFixed(2)}%`;
}

interface EditForm {
  name: string;
  ticker: string;
  assetClass: string;
  amount: number;
}

interface AddForm {
  name: string;
  ticker: string;
  assetClass: string;
  amount: number;
  quantity: string;
  price: string;
}

const emptyAddForm: AddForm = {
  name: "",
  ticker: "",
  assetClass: "Stock",
  amount: 0,
  quantity: "",
  price: "",
};

export function Positions({ positions, currency, onAdd, onUpdate, onDelete }: PositionsProps) {
  const [editing, setEditing] = useState<Position | null>(null);
  const [editForm, setEditForm] = useState<EditForm>({
    name: "",
    ticker: "",
    assetClass: "",
    amount: 0,
  });
  const [adding, setAdding] = useState(false);
  const [addForm, setAddForm] = useState<AddForm>(emptyAddForm);
  const [submitting, setSubmitting] = useState(false);

  const startEdit = (position: Position) => {
    setEditing(position);
    setEditForm({
      name: position.name || "",
      ticker: position.ticker || "",
      assetClass: position.assetClass,
      amount: position.amount,
    });
  };

  const saveEdit = async () => {
    if (!editing) return;
    setSubmitting(true);
    try {
      await onUpdate(editing.id, {
        name: editForm.name || null,
        ticker: editForm.ticker || null,
        assetClass: editForm.assetClass,
        amount: editForm.amount,
      });
      setEditing(null);
    } catch (e) {
      console.error("Failed to save position:", e);
    } finally {
      setSubmitting(false);
    }
  };

  const saveAdd = async () => {
    setSubmitting(true);
    try {
      const quantity = parseFloat(addForm.quantity);
      const price = parseFloat(addForm.price);
      const hasPurchase = !isNaN(quantity) && quantity > 0;
      await onAdd({
        name: addForm.name.trim() || null,
        ticker: addForm.ticker.trim() || null,
        assetClass: addForm.assetClass.trim() || "Stock",
        amount: addForm.amount,
        purchases: hasPurchase
          ? [
              {
                date: null,
                quantity,
                price: isNaN(price) ? null : price,
                fees: null,
              },
            ]
          : [],
      });
      setAdding(false);
      setAddForm(emptyAddForm);
    } catch (e) {
      console.error("Failed to add position:", e);
    } finally {
      setSubmitting(false);
    }
  };

  return (
    <div className="space-y-6">
      <div className="flex items-center justify-between">
        <div>
          <h1 className="text-2xl font-bold tracking-tight">Positions</h1>
          <p className="text-[var(--fg-muted)]">Manage your holdings</p>
        </div>
        <Button onClick={() => setAdding(true)}>
          <Plus className="h-4 w-4" />
          Add Position
        </Button>
      </div>

      <Card>
        <CardHeader>
          <CardTitle>Holdings</CardTitle>
          <CardDescription>{positions.length} positions</CardDescription>
        </CardHeader>
        <CardContent>
          <Table>
            <TableHeader>
              <TableRow>
                <TableHead>Name</TableHead>
                <TableHead>Class</TableHead>
                <TableHead>Amount</TableHead>
                <TableHead>Price</TableHead>
                <TableHead>Value</TableHead>
                <TableHead>PnL</TableHead>
                <TableHead>Day</TableHead>
                <TableHead className="text-right">Actions</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {positions.map((position) => (
                <TableRow key={position.id}>
                  <TableCell>
                    <div>
                      <p className="font-medium">{position.name || position.ticker || "Unknown"}</p>
                      {position.ticker && position.name && (
                        <p className="text-xs text-[var(--fg-muted)]">{position.ticker}</p>
                      )}
                    </div>
                  </TableCell>
                  <TableCell>
                    <Badge variant="default">{position.assetClass}</Badge>
                  </TableCell>
                  <TableCell>{position.amount.toFixed(4)}</TableCell>
                  <TableCell>
                    {position.price > 0 ? formatCurrency(position.price, currency) : "-"}
                  </TableCell>
                  <TableCell className="font-medium">
                    {formatCurrency(position.value, currency)}
                  </TableCell>
                  <TableCell
                    className={
                      (position.pnlPercent ?? 0) >= 0
                        ? "text-[var(--success)]"
                        : "text-[var(--danger)]"
                    }
                  >
                    {formatPercent(position.pnlPercent)}
                  </TableCell>
                  <TableCell
                    className={
                      (position.dayChangePercent ?? 0) >= 0
                        ? "text-[var(--success)]"
                        : "text-[var(--danger)]"
                    }
                  >
                    {formatPercent(position.dayChangePercent)}
                  </TableCell>
                  <TableCell className="text-right">
                    <div className="flex justify-end gap-2">
                      <Button variant="ghost" size="icon" onClick={() => startEdit(position)}>
                        <Edit className="h-4 w-4" />
                      </Button>
                      <Button variant="ghost" size="icon" onClick={() => onDelete(position.id)}>
                        <Trash2 className="h-4 w-4 text-[var(--danger)]" />
                      </Button>
                    </div>
                  </TableCell>
                </TableRow>
              ))}
            </TableBody>
          </Table>
        </CardContent>
      </Card>

      <Dialog
        open={!!editing}
        onOpenChange={(open) => !open && setEditing(null)}
        title="Edit Position"
      >
        <div className="space-y-4">
          <div className="grid grid-cols-2 gap-4">
            <div className="space-y-1">
              <label className="text-xs font-medium text-[var(--fg-muted)]">Name</label>
              <Input
                value={editForm.name}
                onChange={(e) => setEditForm({ ...editForm, name: e.target.value })}
              />
            </div>
            <div className="space-y-1">
              <label className="text-xs font-medium text-[var(--fg-muted)]">Ticker</label>
              <Input
                value={editForm.ticker}
                onChange={(e) => setEditForm({ ...editForm, ticker: e.target.value })}
              />
            </div>
          </div>
          <div className="grid grid-cols-2 gap-4">
            <div className="space-y-1">
              <label className="text-xs font-medium text-[var(--fg-muted)]">Asset Class</label>
              <Input
                value={editForm.assetClass}
                onChange={(e) => setEditForm({ ...editForm, assetClass: e.target.value })}
              />
            </div>
            <div className="space-y-1">
              <label className="text-xs font-medium text-[var(--fg-muted)]">Amount</label>
              <Input
                type="number"
                value={editForm.amount}
                onChange={(e) => setEditForm({ ...editForm, amount: parseFloat(e.target.value) })}
              />
            </div>
          </div>
          <div className="flex justify-end gap-2 pt-2">
            <Button variant="secondary" onClick={() => setEditing(null)}>
              Cancel
            </Button>
            <Button onClick={saveEdit} disabled={submitting}>
              Save Changes
            </Button>
          </div>
        </div>
      </Dialog>

      <Dialog
        open={adding}
        onOpenChange={(open) => !open && setAdding(false)}
        title="Add Position"
        description="Create a new holding. An optional initial purchase records your cost basis."
      >
        <div className="space-y-4">
          <div className="grid grid-cols-2 gap-4">
            <div className="space-y-1">
              <label className="text-xs font-medium text-[var(--fg-muted)]">Name</label>
              <Input
                value={addForm.name}
                placeholder="Apple"
                onChange={(e) => setAddForm({ ...addForm, name: e.target.value })}
              />
            </div>
            <div className="space-y-1">
              <label className="text-xs font-medium text-[var(--fg-muted)]">Ticker</label>
              <Input
                value={addForm.ticker}
                placeholder="AAPL"
                onChange={(e) => setAddForm({ ...addForm, ticker: e.target.value })}
              />
            </div>
          </div>
          <div className="grid grid-cols-2 gap-4">
            <div className="space-y-1">
              <label className="text-xs font-medium text-[var(--fg-muted)]">Asset Class</label>
              <Input
                value={addForm.assetClass}
                onChange={(e) => setAddForm({ ...addForm, assetClass: e.target.value })}
              />
            </div>
            <div className="space-y-1">
              <label className="text-xs font-medium text-[var(--fg-muted)]">Amount</label>
              <Input
                type="number"
                value={addForm.amount}
                onChange={(e) => setAddForm({ ...addForm, amount: parseFloat(e.target.value) })}
              />
            </div>
          </div>
          <div className="rounded-lg border border-[var(--border)] bg-[var(--bg)] p-3">
            <p className="mb-3 text-xs font-medium text-[var(--fg-muted)]">
              Initial purchase (optional — sets cost basis)
            </p>
            <div className="grid grid-cols-2 gap-4">
              <div className="space-y-1">
                <label className="text-xs text-[var(--fg-subtle)]">Quantity</label>
                <Input
                  type="number"
                  value={addForm.quantity}
                  placeholder="0"
                  onChange={(e) => setAddForm({ ...addForm, quantity: e.target.value })}
                />
              </div>
              <div className="space-y-1">
                <label className="text-xs text-[var(--fg-subtle)]">Price ({currency})</label>
                <Input
                  type="number"
                  value={addForm.price}
                  placeholder="0.00"
                  onChange={(e) => setAddForm({ ...addForm, price: e.target.value })}
                />
              </div>
            </div>
          </div>
          <div className="flex justify-end gap-2 pt-2">
            <Button variant="secondary" onClick={() => setAdding(false)}>
              Cancel
            </Button>
            <Button onClick={saveAdd} disabled={submitting}>
              Add Position
            </Button>
          </div>
        </div>
      </Dialog>
    </div>
  );
}
