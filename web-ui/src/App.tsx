import { Navigate, Route, Routes } from "react-router-dom";
import Layout from "@/components/layout/layout";
import SearchPage from "@/routes/search";
import BookDetailPage from "@/routes/book-detail";
import TasksPage from "@/routes/tasks";
import SourcesPage from "@/routes/sources";
import LibraryPage from "@/routes/library";
import SettingsPage from "@/routes/settings";

export default function App() {
  return (
    <Routes>
      <Route element={<Layout />}>
        <Route index element={<Navigate to="/search" replace />} />
        <Route path="search" element={<SearchPage />} />
        <Route path="search/:bookUrl" element={<BookDetailPage />} />
        <Route path="tasks" element={<TasksPage />} />
        <Route path="library" element={<LibraryPage />} />
        <Route path="sources" element={<SourcesPage />} />
        <Route path="settings" element={<SettingsPage />} />
      </Route>
    </Routes>
  );
}
