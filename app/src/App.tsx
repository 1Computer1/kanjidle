import Puzzle from "./components/Puzzle";

export default function App() {
  return (
    <div className="h-screen w-screen overflow-y-scroll bg-zinc-100 text-2xl text-zinc-900 lg:text-3xl xl:text-4xl dark:bg-zinc-900 dark:text-zinc-100">
      <div className="flex w-full flex-col items-center justify-center py-4">
        <h1>Kanjidle・漢字パズル</h1>
      </div>
      <Puzzle />
    </div>
  );
}
