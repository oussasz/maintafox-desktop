/**
 * Choice from one of the plant's personnel lists (Données de référence). Values flagged
 * "à revoir" are offered only when already chosen (the record keeps its value until reviewed);
 * a stored code that is not in the list stays visible with its exact code. Without the right to
 * read the lists, or while they fail to load, the field says why instead of showing nothing.
 */
import { useTranslation } from "react-i18next";

import { ErrorState } from "@/components/ui/data-state";
import { ListAccessNote } from "@/components/ui/list-access-note";
import { ReviewMark } from "@/components/ui/review-mark";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import type { PersonnelListName } from "@/stores/personnel-lists-store";

import type { PersonnelLists } from "./use-personnel-lists";

export function PersonnelListSelect({
  list,
  lists,
  value,
  onChange,
  id,
  disabled,
  ariaLabel,
}: {
  list: PersonnelListName;
  lists: PersonnelLists;
  value: string;
  onChange: (code: string) => void;
  id?: string;
  disabled?: boolean;
  ariaLabel?: string;
}) {
  const { t } = useTranslation("common");
  if (!lists.canRead) return <ListAccessNote list="people" />;
  if (lists.error != null) {
    return <ErrorState className="py-3" error={lists.error} onRetry={() => void lists.reload()} />;
  }
  const current = lists.option(list, value);
  const choices = lists.values(list).filter((v) => !v.review_required || v.code === value);
  const unknownCurrent = value !== "" && current == null;
  return (
    <div className="space-y-1">
      <Select value={value} onValueChange={onChange} disabled={disabled === true || lists.loading}>
        <SelectTrigger id={id} aria-label={ariaLabel}>
          <SelectValue placeholder={t(lists.loading ? "ds.state.loading" : "ds.state.choose")} />
        </SelectTrigger>
        <SelectContent>
          {choices.map((v) => (
            <SelectItem key={v.code} value={v.code}>
              {lists.text(list, v.code)}
            </SelectItem>
          ))}
          {unknownCurrent ? <SelectItem value={value}>{value}</SelectItem> : null}
        </SelectContent>
      </Select>
      {current?.review_required || unknownCurrent ? <ReviewMark /> : null}
    </div>
  );
}
