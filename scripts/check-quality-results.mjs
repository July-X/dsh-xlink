// Keep the existing required-check name while failing closed on every dependency.
import { pathToFileURL } from 'node:url';

const REQUIRED_JOBS = ['quality-js', 'quality-rust'];

export function checkQualityResults(results) {
  return REQUIRED_JOBS.filter((job) => results?.[job]?.result !== 'success');
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  try {
    const failed = checkQualityResults(JSON.parse(process.env.QUALITY_RESULTS ?? 'null'));
    if (failed.length > 0) {
      throw new Error(`Quality jobs did not pass: ${failed.join(', ')}`);
    }
    console.log('All quality checks passed.');
  } catch (error) {
    console.error(error.message);
    process.exitCode = 1;
  }
}
